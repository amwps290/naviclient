//! The Home page's time-of-day heading.

use chrono::Timelike;
use rust_i18n::t;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Greeting {
    Morning,
    Afternoon,
    Evening,
}

fn greeting_for_hour(hour: u32) -> Greeting {
    match hour {
        5..=11 => Greeting::Morning,
        12..=17 => Greeting::Afternoon,
        _ => Greeting::Evening,
    }
}

pub(in crate::app) fn current_greeting() -> std::borrow::Cow<'static, str> {
    match greeting_for_hour(chrono::Local::now().hour()) {
        Greeting::Morning => t!("Good morning"),
        Greeting::Afternoon => t!("Good afternoon"),
        Greeting::Evening => t!("Good evening"),
    }
}

#[cfg(test)]
mod tests;
