use std::fmt;
use std::time;

/// Format a duration as a human-readable time duration.
pub struct DurationFormat<'a>(pub &'a time::Duration);

impl<'a> fmt::Display for DurationFormat<'a> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "{}", self.0.as_secs())?;

        let nanos = self.0.subsec_nanos();

        if nanos > 1_000_000 {
            write!(fmt, ".{}", (nanos / 1_000_000) % 1_000)?;
        }

        write!(fmt, "s")?;
        Ok(())
    }
}
