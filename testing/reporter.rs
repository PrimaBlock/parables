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

pub trait Reporter: Sync {
    /// Report a single result.
    fn report(&self, result: TestResult) -> Result<(), Error>;

    /// Report that a number of tests have been skipped.
    fn report_skipped(&self, test: Test) -> Result<(), Error>;

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

impl Reporter for StdoutReporter {
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
