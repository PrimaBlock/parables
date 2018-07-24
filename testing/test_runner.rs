//! Provides a simple test scaffolding for running tests in parallel.
use error::Error;
use reporter::Reporter;
use std::any;
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::panic;
use std::sync::{atomic, Arc, Mutex};
use std::thread;
use std::time;

/// Convert into a result.
pub trait IntoResult<T>: Send {
    fn into_result(self) -> Result<T, Error>;
}

impl IntoResult<()> for Result<(), Error> {
    fn into_result(self) -> Result<(), Error> {
        self
    }
}

impl IntoResult<()> for () {
    fn into_result(self) -> Result<(), Error> {
        Ok(())
    }
}

/// The entrypoint of a test.
pub trait TestEntry: Send {
    fn run(&self) -> Result<(), Error>;
}

/// A test function, that might return a result.
impl<F, T> TestEntry for F
where
    F: Fn() -> T + Send,
    T: IntoResult<()>,
{
    fn run(&self) -> Result<(), Error> {
        (self)().into_result()
    }
}

/// An empty test.
impl TestEntry for () {
    fn run(&self) -> Result<(), Error> {
        Ok(())
    }
}

/// A single test.
pub struct Test<'a> {
    /// Module of the test.
    pub(crate) module: Option<Cow<'a, str>>,
    /// Name of the test.
    pub(crate) name: Cow<'a, str>,
    /// Entry-point to the test. Must be guarded against panics, since that is how Rust asserts
    /// work.
    pub(crate) entry: Box<'a + TestEntry>,
}

impl<'a> Test<'a> {
    /// Access the name of the test.
    pub fn name(&self) -> &str {
        self.name.as_ref()
    }
}

impl<'a> fmt::Debug for Test<'a> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        fmt.debug_struct("Test").field("name", &self.name).finish()
    }
}

/// Information about a panic.
#[derive(Clone, Default, Hash, PartialEq, Eq)]
pub struct PanicInfo {
    pub(crate) location: Option<Location>,
    pub(crate) message: Option<String>,
}

/// Location of a panic.
#[derive(Clone, Hash, PartialEq, Eq)]
pub struct Location {
    pub(crate) file: String,
    pub(crate) line: u32,
    pub(crate) column: u32,
}

impl<'a, 'b: 'a> From<&'a panic::Location<'b>> for Location {
    fn from(value: &'a panic::Location<'b>) -> Self {
        Location {
            file: value.file().to_string(),
            line: value.line(),
            column: value.column(),
        }
    }
}

/// The outcome of a single test.
#[derive(PartialEq)]
pub enum Outcome {
    /// Contains information about the failed outcome.
    Failed(PanicInfo),
    /// An error was raised.
    Errored(Error),
    /// Only indicates that the test was successful.
    Ok,
}

/// The result from a single test.
pub struct TestResult<'a> {
    /// The module that the test belonged to.
    pub(crate) module: Option<Cow<'a, str>>,
    /// Name of the test the result refers to.
    pub(crate) name: Cow<'a, str>,
    /// The outcome of the test.
    pub(crate) outcome: Outcome,
    /// Duration that the test was running for.
    pub(crate) duration: time::Duration,
}

impl<'a> TestResult<'a> {
    /// Access the name of the test results.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Access the outcome of the test.
    pub fn outcome(&self) -> &Outcome {
        &self.outcome
    }

    /// Access the duration of the test.
    pub fn duration(&self) -> &time::Duration {
        &self.duration
    }
}

/// Helper trait to register tests.
pub trait Suite<'a> {
    /// Register a single test, with a human-readable `name`.
    fn test<N: Into<Cow<'a, str>>, F: 'a, T>(&mut self, name: N, entry: F)
    where
        F: Fn() -> T + Send,
        T: IntoResult<()>;
}

/// A scaffolding that runs tests very efficiently.
#[derive(Debug)]
pub struct TestRunner<'a> {
    tests: Vec<Test<'a>>,
}

impl<'a> TestRunner<'a> {
    /// Build a new test runner.
    pub fn new() -> Self {
        Self { tests: Vec::new() }
    }

    /// Create a module runner.
    pub fn module<'m>(&'m mut self, name: impl Into<Cow<'a, str>>) -> ModuleRunner<'m, 'a> {
        ModuleRunner {
            test_runner: self,
            name: name.into(),
        }
    }

    /// Run by reading filters from argv.
    pub fn run(self, reporter: &Reporter<'a>) -> Result<(), Error> {
        use std::env;

        let mut args = env::args();
        args.next();

        self.run_with_filters(args.collect::<HashSet<String>>(), reporter)
    }

    fn run_in_parallel(reporter: &Reporter<'a>, tests: Vec<Test<'a>>, done: impl FnOnce()) {
        use rayon::prelude::*;

        let catch = Arc::new(Mutex::new(HashMap::new()));
        let local_catch = catch.clone();

        panic::set_hook(Box::new(move |info| {
            let id = thread::current().id();

            let mut catch = local_catch.lock().expect("poisoned lock");
            let catch = catch.entry(id).or_insert_with(PanicInfo::default);

            catch.location = info.location().map(Location::from);
            catch.message = payload_to_message(info.payload());
        }));

        let index = atomic::AtomicUsize::new(0usize);

        let results = tests.into_par_iter().map(|test| {
            let index = index.fetch_add(1usize, atomic::Ordering::Relaxed);

            match reporter.report_started(index, &test.name) {
                Err(e) => println!("error in reporting: {}", e),
                Ok(()) => {}
            }

            (index, Self::run_one_test(test, catch.clone()))
        });

        results.for_each(|(index, r)| match reporter.report(index, r) {
            Err(e) => println!("error in reporting: {}", e),
            Ok(()) => {}
        });

        let _ = panic::take_hook();

        done();

        /// downcast the info payload to a string message.
        fn payload_to_message(any: &any::Any) -> Option<String> {
            if let Some(string) = any.downcast_ref::<&'static str>() {
                return Some(string.to_string());
            }

            if let Some(string) = any.downcast_ref::<String>() {
                return Some(string.to_string());
            }

            None
        }
    }

    /// Run all registered tests, while applying the given filter on their name.
    ///
    /// All strings specified in the filter must be apart of the name of a test to include it.
    ///
    /// Note: this installs a panic hook, so mixing this with another component that fiddles with
    /// the hook will cause unexpected results.
    pub fn run_with_filters<F>(self, filters: F, reporter: &Reporter<'a>) -> Result<(), Error>
    where
        F: IntoIterator<Item = String>,
    {
        use rayon;

        let filters = filters.into_iter().collect::<HashSet<_>>();

        let mut tests = Vec::new();

        for test in self.tests {
            let matches_module =
                |test: &Test, f| test.module.as_ref().map(|m| m == f).unwrap_or(false);

            if filters
                .iter()
                .all(|f| test.name.contains(f) || matches_module(&test, f))
            {
                tests.push(test);
            } else {
                reporter.report_skipped(test)?;
            }
        }

        let done = atomic::AtomicBool::new(false);

        if reporter.supports_animation()? {
            rayon::scope(|s| {
                s.spawn(|s| {
                    s.spawn(|_| {
                        while !done.load(atomic::Ordering::Acquire) {
                            thread::sleep(time::Duration::from_millis(100));
                            reporter.animate().expect("animation failed");
                        }
                    });

                    Self::run_in_parallel(reporter, tests, || {
                        done.store(true, atomic::Ordering::Release)
                    });
                });
            });
        } else {
            Self::run_in_parallel(reporter, tests, || {});
        }

        reporter.end()?;
        return Ok(());
    }

    /// Internal function to register a test.
    fn internal_test<N: Into<Cow<'a, str>>, F: 'a, T>(
        &mut self,
        module: Option<Cow<'a, str>>,
        name: N,
        entry: F,
    ) where
        F: Fn() -> T + Send,
        T: IntoResult<()>,
    {
        self.tests.push(Test {
            module,
            name: name.into(),
            entry: Box::new(entry),
        })
    }

    /// Run a single test.
    fn run_one_test(
        test: Test<'a>,
        catch: Arc<Mutex<HashMap<thread::ThreadId, PanicInfo>>>,
    ) -> TestResult<'a> {
        let Test {
            module,
            name,
            entry,
            ..
        } = test;
        let start = time::Instant::now();
        let res = panic::catch_unwind(panic::AssertUnwindSafe(move || entry.run()));
        let end = time::Instant::now();
        let duration = end.duration_since(start);

        let out = match res {
            Err(_) => {
                let id = thread::current().id();

                let mut catch = catch.lock().expect("poisoned lock");
                let mut catch = catch.remove(&id).unwrap_or_else(PanicInfo::default);

                TestResult {
                    module,
                    name,
                    outcome: Outcome::Failed(catch),
                    duration,
                }
            }
            Ok(Err(e)) => TestResult {
                module,
                name,
                outcome: Outcome::Errored(e),
                duration,
            },
            Ok(Ok(())) => TestResult {
                module,
                name,
                outcome: Outcome::Ok,
                duration,
            },
        };

        return out;
    }
}

impl<'a> Suite<'a> for TestRunner<'a> {
    fn test<N: Into<Cow<'a, str>>, F: 'a, T>(&mut self, name: N, entry: F)
    where
        F: Fn() -> T + Send,
        T: IntoResult<()>,
    {
        self.internal_test(None, name, entry)
    }
}

pub struct ModuleRunner<'m, 'a: 'm> {
    test_runner: &'m mut TestRunner<'a>,
    name: Cow<'a, str>,
}

impl<'m, 'a: 'm> Suite<'a> for ModuleRunner<'m, 'a> {
    fn test<N: Into<Cow<'a, str>>, F: 'a, T>(&mut self, name: N, entry: F)
    where
        F: Fn() -> T + Send,
        T: IntoResult<()>,
    {
        self.test_runner
            .internal_test(Some(self.name.clone()), name, entry)
    }
}

#[cfg(test)]
mod tests {
    use super::Suite;
    use super::{Outcome, TestRunner};
    use reporter::CollectingReporter;
    use std::collections::HashMap;
    use std::iter;

    #[test]
    pub fn test_runner() {
        let mut runner = TestRunner::new();
        runner.test("my failure", my_failure);
        runner.test("my success", my_success);

        let reporter = CollectingReporter::new();
        runner
            .run_with_filters(iter::empty(), &reporter)
            .expect("tests to run");
        let result = reporter.take_results().expect("bad results");

        let result = result
            .into_iter()
            .map(|result| (result.name.to_string(), result))
            .collect::<HashMap<_, _>>();

        assert_eq!(
            Some(&Outcome::Ok),
            result.get("my success").map(|r| &r.outcome)
        );

        match result.get("my failure").map(|r| &r.outcome) {
            Some(&Outcome::Failed(ref info)) => {
                assert!(info.location.is_some());
                assert_eq!(
                    Some("my_failure_message"),
                    info.message.as_ref().map(|m| m.as_str())
                );
            }
            _ => panic!("expected failure outcome"),
        }

        fn my_failure() {
            assert!(false, "my_failure_message");
        }

        fn my_success() {
            assert!(true);
        }
    }

    #[test]
    pub fn test_module() {
        let mut runner = TestRunner::new();

        {
            let m = runner.module("deposit");
        }
    }
}
