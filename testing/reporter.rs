use error::Error;
use std::sync::Mutex;
use test_runner::{Outcome, Test, TestResult};

#[derive(Debug, Default)]
pub struct Account {
    count: u32,
    passed: u32,
    failed: u32,
    skipped: u32,
}

pub trait Reporter<'a>: Sync {
    /// Report a single result.
    fn report(&self, result: TestResult<'a>) -> Result<(), Error>;

    /// Report that a number of tests have been skipped.
    fn report_skipped(&self, test: Test<'a>) -> Result<(), Error>;

    /// Close the reporter.
    fn close(&self) -> Result<(), Error>;
}

/// A components that prints test results.
pub struct StdoutReporter {
    account: Mutex<Account>,
}

impl StdoutReporter {
    pub fn new() -> Self {
        Self {
            account: Mutex::new(Account::default()),
        }
    }
}

impl<'a> Reporter<'a> for StdoutReporter {
    fn report(&self, result: TestResult) -> Result<(), Error> {
        println!("{:?}", result);

        let mut account = self.account.lock().map_err(|_| "lock poisoned")?;
        account.count += 1;

        match *result.outcome() {
            Outcome::Ok => account.passed += 1,
            _ => account.failed += 1,
        }

        Ok(())
    }

    fn report_skipped(&self, test: Test) -> Result<(), Error> {
        println!("{}: skipped", test.name());

        let mut account = self.account.lock().map_err(|_| "lock poisoned")?;
        account.count += 1;
        account.skipped += 1;
        Ok(())
    }

    fn close(&self) -> Result<(), Error> {
        let account = self.account.lock().map_err(|_| "lock poisoned")?;

        let outcome = if account.failed == 0 { "ok" } else { "failed" };

        println!(
            "test result: {}. {passed} passed; {failed} failed; {skipped} skipped",
            outcome,
            passed = account.passed,
            failed = account.failed,
            skipped = account.skipped,
        );

        Ok(())
    }
}

/// A reporter that doesn't report anything.
pub struct CollectingReporter<'a> {
    results: Mutex<Vec<TestResult<'a>>>,
}

impl<'a> CollectingReporter<'a> {
    pub fn new() -> Self {
        Self {
            results: Mutex::new(Vec::new()),
        }
    }

    /// Take all collected results.
    pub fn take_results(self) -> Result<Vec<TestResult<'a>>, Error> {
        Ok(self.results
            .into_inner()
            .map_err(|_| "another lock is held")?)
    }
}

impl<'a> Reporter<'a> for CollectingReporter<'a> {
    fn report(&self, result: TestResult<'a>) -> Result<(), Error> {
        let mut results = self.results.lock().map_err(|_| "lock poisoned")?;
        results.push(result);
        Ok(())
    }

    fn report_skipped(&self, _test: Test<'a>) -> Result<(), Error> {
        Ok(())
    }

    fn close(&self) -> Result<(), Error> {
        Ok(())
    }
}
