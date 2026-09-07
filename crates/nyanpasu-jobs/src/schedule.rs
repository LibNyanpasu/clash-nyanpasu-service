use crate::Error;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use croner::parser::{CronParser, Seconds, Year};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Schedule {
    #[default]
    Manual,
    Once {
        delay_ms: u64,
    },
    Interval {
        every_ms: u64,
    },
    Cron {
        expression: String,
        timezone: String,
    },
}
impl Schedule {
    pub fn validate(&self, now: DateTime<Utc>) -> Result<(), Error> {
        match self {
            Self::Interval { every_ms: 0 } => Err(Error::Invalid("zero interval".into())),
            Self::Once { delay_ms } | Self::Interval { every_ms: delay_ms }
                if *delay_ms > 10 * 365 * 86400 * 1000 =>
            {
                Err(Error::Invalid("delay exceeds ten years".into()))
            }
            Self::Cron { .. } => self.next_cron(now).map(|_| ()),
            _ => Ok(()),
        }
    }
    pub fn next_cron(&self, after: DateTime<Utc>) -> Result<DateTime<Utc>, Error> {
        let Self::Cron {
            expression,
            timezone,
        } = self
        else {
            return Err(Error::Invalid("not cron".into()));
        };
        let timezone: Tz = timezone
            .parse()
            .map_err(|_| Error::Invalid("unknown IANA timezone".into()))?;
        let cron = CronParser::builder()
            .seconds(Seconds::Optional)
            .year(Year::Disallowed)
            .build()
            .parse(expression)
            .map_err(|e| Error::Invalid(e.to_string()))?;
        cron.find_next_occurrence(&after.with_timezone(&timezone), false)
            .map(|t| t.with_timezone(&Utc))
            .map_err(|e| Error::Invalid(e.to_string()))
    }
}
