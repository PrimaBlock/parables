use failure::Error;
use isatty;
use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::sync::Mutex;
use term;
use test_runner::{Location, Outcome, Test, TestResult};
use utils;

#[derive(Debug)]
pub enum Step {
    North,
    NorthEast,
    East,
    SouthEast,
}

impl Default for Step {
    fn default() -> Self {
        Step::North
    }
}

impl Step {
    /// Translate to next step.
    fn next(&mut self) {
        let next = match *self {
            Step::North => Step::NorthEast,
            Step::NorthEast => Step::East,
            Step::East => Step::SouthEast,
            Step::SouthEast => Step::North,
        };

        *self = next;
    }

    /// Render the current step.
    fn render(&self) -> &'static str {
        match *self {
            Step::North => "|",
            Step::NorthEast => "/",
            Step::East => "-",
            Step::SouthEast => "\\",
        }
    }
}

#[derive(Debug, Default)]
pub struct Account {
    count: u32,
    passed: u32,
    failed: u32,
    skipped: u32,
    running: BTreeMap<usize, String>,
    step: Step,
}

pub trait Reporter<'a>: Sync {
    /// Check if reporter supports animation.
    fn supports_animation(&self) -> Result<bool, Error> {
        Ok(false)
    }

    /// Animate the reporter.
    fn animate(&self) -> Result<(), Error> {
        Ok(())
    }

    /// End any in-progress animations.
    fn end(&self) -> Result<(), Error> {
        Ok(())
    }

    /// Report that we've started running a test.
    fn report_started(&self, _index: usize, _name: &str) -> Result<(), Error> {
        Ok(())
    }

    /// Report a single result.
    fn report(&self, index: usize, result: TestResult<'a>) -> Result<(), Error>;

    /// Report that a number of tests have been skipped.
    fn report_skipped(&self, test: Test<'a>) -> Result<(), Error>;

    /// Close the reporter.
    fn close(&self) -> Result<(), Error>;
}

struct ReporterState {
    out: Terminal,
    account: Account,
}

/// A components that prints test results.
pub struct StdoutReporter {
    state: Mutex<ReporterState>,
    report_skipped: bool,
}

impl StdoutReporter {
    pub fn new() -> Result<Self, Error> {
        // make sure terminal is a tty and supports fancy features.
        let out = if isatty::stdout_isatty() {
            match term::stdout() {
                Some(terminal) => {
                    let fancy = terminal.supports_reset() && terminal.supports_color();
                    Coloring::Colored { terminal, fancy }
                }
                None => Coloring::Raw(io::stdout()),
            }
        } else {
            Coloring::Raw(io::stdout())
        };

        let out = Terminal { output: out };

        Ok(Self {
            state: Mutex::new(ReporterState {
                out,
                account: Account::default(),
            }),
            report_skipped: false,
        })
    }

    /// Configure reporter to report skipped.
    pub fn report_skipped(self) -> Self {
        Self {
            report_skipped: true,
            ..self
        }
    }

    /// Report progress to the given terminal.
    fn report_progress(&self, out: &mut Terminal, account: &mut Account) -> Result<(), Error> {
        let mut names = account
            .running
            .values()
            .take(2)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");

        if account.running.len() > 2 {
            names.push_str(", ...");
        }

        write!(
            out,
            "{} {} running: {} {}",
            account.running.len(),
            if account.running.len() == 1 {
                "test"
            } else {
                "tests"
            },
            names,
            account.step.render(),
        )?;
        out.flush()?;
        Ok(())
    }
}

impl<'a> Reporter<'a> for StdoutReporter {
    fn supports_animation(&self) -> Result<bool, Error> {
        let mut state = self.state.lock().map_err(|_| format_err!("lock poisoned"))?;

        let ReporterState { ref mut out, .. } = *state;

        Ok(out.is_fancy())
    }

    fn animate(&self) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| format_err!("lock poisoned"))?;

        let ReporterState {
            ref mut out,
            ref mut account,
        } = *state;

        // only report if fancy and can clear lines.
        if out.is_fancy() {
            out.clear_line()?;
            self.report_progress(out, account)?;
            account.step.next();
        }

        Ok(())
    }

    fn end(&self) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| format_err!("lock poisoned"))?;

        let ReporterState { ref mut out, .. } = *state;

        // only report if fancy and can clear lines.
        if out.is_fancy() {
            out.clear_line()?;
        }

        Ok(())
    }

    fn report_started(&self, index: usize, name: &str) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| format_err!("lock poisoned"))?;

        let ReporterState {
            ref mut out,
            ref mut account,
        } = *state;

        account.running.insert(index, name.to_string());

        // only report if fancy and can clear lines.
        if out.is_fancy() {
            out.clear_line()?;
            self.report_progress(out, account)?;
        }

        Ok(())
    }

    fn report(&self, index: usize, result: TestResult) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| format_err!("lock poisoned"))?;

        let ReporterState {
            ref mut out,
            ref mut account,
        } = *state;

        if out.is_fancy() {
            out.clear_line()?;
        }

        account.running.remove(&index);
        account.count += 1;

        ColoredTestResult(&result).fmt(out)?;

        match *result.outcome() {
            Outcome::Ok => account.passed += 1,
            _ => account.failed += 1,
        }

        if out.is_fancy() {
            self.report_progress(out, account)?;
        }

        Ok(())
    }

    fn report_skipped(&self, test: Test) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| format_err!("lock poisoned"))?;

        let ReporterState {
            ref mut out,
            ref mut account,
        } = *state;

        if self.report_skipped {
            write!(out, "{}: ", test.name())?;
            out.yellow("skipped")?;
            writeln!(out)?;
        }

        account.count += 1;
        account.skipped += 1;
        Ok(())
    }

    fn close(&self) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| format_err!("lock poisoned"))?;

        let ReporterState {
            ref mut out,
            ref mut account,
        } = *state;

        write!(out, "test result: ")?;

        if account.failed == 0 {
            out.green("OK")?;
        } else {
            out.red("FAILED")?;
        }

        write!(out, ". ")?;
        out.green(account.passed)?;
        write!(out, " passed; ")?;
        out.red(account.failed)?;
        write!(out, " failed; ")?;
        out.yellow(account.skipped)?;
        write!(out, " skipped")?;
        writeln!(out)?;
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
        Ok(self
            .results
            .into_inner()
            .map_err(|_| format_err!("another lock is held"))?)
    }
}

impl<'a> Reporter<'a> for CollectingReporter<'a> {
    fn report(&self, _index: usize, result: TestResult<'a>) -> Result<(), Error> {
        let mut results = self
            .results
            .lock()
            .map_err(|_| format_err!("lock poisoned"))?;
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

enum Coloring {
    Colored {
        terminal: Box<term::StdoutTerminal>,
        fancy: bool,
    },
    Raw(io::Stdout),
}

impl Coloring {
    fn fg(&mut self, color: u32) -> fmt::Result {
        if let Coloring::Colored {
            ref mut terminal,
            fancy,
            ..
        } = *self
        {
            if fancy {
                terminal.fg(color).map_err(|_| fmt::Error)?;
            }
        }

        Ok(())
    }

    fn reset(&mut self) -> fmt::Result {
        if let Coloring::Colored {
            ref mut terminal,
            fancy,
        } = *self
        {
            if fancy {
                terminal.reset().map_err(|_| fmt::Error)?;
            }
        }

        Ok(())
    }

    fn write_fmt(&mut self, fmt: fmt::Arguments) -> fmt::Result {
        use self::io::Write;

        match *self {
            Coloring::Colored {
                ref mut terminal, ..
            } => terminal.write_fmt(fmt).map_err(|_| fmt::Error),
            Coloring::Raw(ref mut stdout) => stdout.write_fmt(fmt).map_err(|_| fmt::Error),
        }
    }

    fn is_fancy(&self) -> bool {
        match *self {
            Coloring::Colored { fancy, .. } => fancy,
            Coloring::Raw(..) => false,
        }
    }

    fn flush(&mut self) -> fmt::Result {
        use self::io::Write;

        match *self {
            Coloring::Colored {
                ref mut terminal, ..
            } => terminal.flush().map_err(|_| fmt::Error),
            Coloring::Raw(ref mut stdout) => stdout.flush().map_err(|_| fmt::Error),
        }
    }

    fn clear_line(&mut self) -> fmt::Result {
        match *self {
            Coloring::Colored {
                ref mut terminal, ..
            } => {
                terminal.carriage_return().map_err(|_| fmt::Error)?;
                terminal.delete_line().map_err(|_| fmt::Error)?;
                Ok(())
            }
            Coloring::Raw(..) => return Err(fmt::Error),
        }
    }
}

struct Terminal {
    output: Coloring,
}

macro_rules! color {
    ($name:ident, $member:ident) => {
        pub fn $name(&mut self, item: impl fmt::Display) -> fmt::Result {
            self.output.fg(term::color::$member)?;
            write!(self, "{}", item)?;
            self.output.reset()?;
            Ok(())
        }
    }
}

impl Terminal {
    fn write_fmt(&mut self, fmt: fmt::Arguments) -> fmt::Result {
        self.output.write_fmt(fmt).map_err(|_| fmt::Error)
    }

    fn is_fancy(&self) -> bool {
        self.output.is_fancy()
    }

    fn flush(&mut self) -> fmt::Result {
        self.output.flush()
    }

    fn clear_line(&mut self) -> fmt::Result {
        self.output.clear_line()
    }

    color!(red, RED);
    color!(green, GREEN);
    color!(yellow, YELLOW);
}

struct ColoredTestResult<'t, 'a: 't>(&'t TestResult<'a>);

impl<'t, 'a: 't> ColoredTestResult<'t, 'a> {
    fn fmt(&self, fmt: &mut Terminal) -> fmt::Result {
        let result = self.0;

        ColoredOutcome(&result.outcome).fmt(fmt)?;

        if let Some(ref module) = result.module {
            write!(fmt, " {} ::", module)?;
        }

        writeln!(
            fmt,
            " {} (took {})",
            result.name,
            utils::DurationFormat(&result.duration)
        )?;

        ColoredOutcome(&result.outcome).fmt_errors(fmt)?;

        Ok(())
    }
}

struct ColoredOutcome<'a>(&'a Outcome);

impl<'a> ColoredOutcome<'a> {
    fn fmt(&self, fmt: &mut Terminal) -> fmt::Result {
        let outcome = self.0;

        match outcome {
            Outcome::Failed(..) => fmt.red("FAIL")?,
            Outcome::Errored(..) => fmt.red("ERROR")?,
            Outcome::Ok => fmt.green("OK")?,
        }

        Ok(())
    }

    fn fmt_errors(&self, fmt: &mut Terminal) -> fmt::Result {
        let outcome = self.0;

        match outcome {
            Outcome::Failed(ref info) => {
                write!(fmt, "Failed at ")?;

                match info.location {
                    Some(ref location) => {
                        ColoredLocation(location).fmt(fmt)?;
                        writeln!(fmt, "")?;
                    }
                    None => {
                        writeln!(fmt, "unknown location")?;
                    }
                }

                if let Some(ref message) = info.message {
                    writeln!(fmt, "{}", message)?;
                }
            }
            Outcome::Errored(ref e) => {
                writeln!(fmt, "{}", e)?;
                writeln!(fmt, "{:?}", e.backtrace())?;

                let mut causes = e.causes().skip(1);

                while let Some(e) = causes.next() {
                    writeln!(fmt, "caused by: {}", e)?;
                    writeln!(fmt, "{:?}", e.backtrace())?;
                }
            }
            _ => {}
        }

        Ok(())
    }
}

struct ColoredLocation<'a>(&'a Location);

impl<'a> ColoredLocation<'a> {
    fn fmt(&self, fmt: &mut Terminal) -> fmt::Result {
        let loc = self.0;
        write!(fmt, "{}:{}:{}", loc.file, loc.line, loc.column)
    }
}
