/*
OUTPUT EXAMPLE:

FLASH_1 used:  170.82 KiB  /  512.00 KiB  (33%)
▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░
.text:         124.68 KiB   +24           (24%)



THE FORMAT
====================

            FLASH_1 used:  170.82 KiB  /  512.00 KiB  (33%)
            DTCMRAM used:  128.00 KiB  /  128.00 KiB (100%)
            ^           ^                          ^      ^
            +-----------+                          |      |
            TITLE_WIDTH                            |      |
                                                   |      |
                         ^                         |      |
                         +-------------------------+      |
                              SIZE_INFO_WIDTH             |
                                                    ^     |
                                                    +-----+
                                                    USAGE_WIDTH

*/

// tweakable
const TITLE_WIDTH: usize = 18;
const USAGE_WIDTH: usize = 7;
const SINGLE_SIZE_WIDTH: usize = 10;
const TITLE_SUFFIX: &str = ": ";

// computed
const SIZE_INFO_WIDTH: usize = 2 * SINGLE_SIZE_WIDTH + 5;
const FULL_LINE_WIDTH: usize = TITLE_WIDTH + SIZE_INFO_WIDTH + USAGE_WIDTH;

fn size_info_section<T, U>(used_space: T, separator: &str, total_space: U) -> String
where
    T: std::fmt::Display,
    U: std::fmt::Display,
{
    format!(
        "{:>SINGLE_SIZE_WIDTH$}  {:1}  {:<SINGLE_SIZE_WIDTH$}",
        used_space, separator, total_space
    )
}

fn region_format<T, U>(title: &str, used_space: T, total_space: U, percent: u8) -> String
where
    T: std::fmt::Display,
    U: std::fmt::Display,
{
    let size_info = size_info_section(used_space, "/", total_space);
    let usage = format!("({}%)", percent);
    let title = title_format(title);
    format!(
        "{:<TITLE_WIDTH$}{}{:>USAGE_WIDTH$}",
        title, size_info, usage
    )
}

fn title_format(title: &str) -> String {
    let max_title_length = TITLE_WIDTH - TITLE_SUFFIX.chars().count();

    if title.chars().count() > max_title_length {
        format!(
            "{:.ellipsisized_title_length$}…{}",
            title,
            TITLE_SUFFIX,
            ellipsisized_title_length = max_title_length - 1
        )
    } else {
        format!("{:.max_title_length$}{}", title, TITLE_SUFFIX)
    }
}

fn diff_section_format<T>(
    section_name: &str,
    used_space: u32,
    diff_string: T,
    percent: u8,
) -> String
where
    T: std::fmt::Display,
{
    let size_info = size_info_section(sizeof_fmt(used_space), "", diff_string);
    let usage = format!("({}%)", percent);
    let title = title_format(section_name);
    format!(
        "{:<TITLE_WIDTH$}{}{:>USAGE_WIDTH$}",
        title, size_info, usage
    )
}

use colored::*;
trait CoolColor {
    fn s_purple(self) -> ColoredString
    where
        Self: Colorize + Sized,
    {
        self.color(Color::TrueColor {
            r: 141,
            g: 128,
            b: 255,
        })
    }

    fn s_pink(self) -> ColoredString
    where
        Self: Colorize + Sized,
    {
        self.color(Color::TrueColor {
            r: 255,
            g: 128,
            b: 221,
        })
    }

    fn mint(self) -> ColoredString
    where
        Self: Colorize + Sized,
    {
        self.color(Color::TrueColor {
            r: 127,
            g: 255,
            b: 191,
        })
    }
}
impl<'a> CoolColor for &'a str {}

use crate::size::Section;
pub fn print_region(name: &str, region_size: u32, sections: &[(Section, i64)]) {
    let total_usage: u32 = sections.iter().fold(0, |acc, x| acc + x.0 .1);
    let size_to_ratio = |size: u32| (size as f32) / (region_size as f32);
    let percent = (100. * size_to_ratio(total_usage)).round() as u8;
    println!(
        "{}",
        region_format(
            format!("{} used", name).as_str(),
            sizeof_fmt(total_usage).s_purple(),
            sizeof_fmt(region_size),
            percent
        )
    );

    let bars = sections
        .iter()
        .map(|&(Section(_, size), _)| {
            let ratio = size_to_ratio(size);
            (ratio * FULL_LINE_WIDTH as f32).round() as u8
        })
        .collect::<Vec<u8>>();

    bars.iter().enumerate().for_each(|(i, val)| {
        let uncolored_bar = (0..*val).map(|_| "▓").collect::<String>();
        match i % 2 == 0 {
            true => print!("{}", uncolored_bar.s_pink()),
            _ => print!("{}", uncolored_bar.s_purple()),
        }
    });

    let used_bars: u8 = bars.iter().sum();
    println!(
        "{}",
        (used_bars..(FULL_LINE_WIDTH as u8))
            .map(|_| "░")
            .collect::<String>()
    );

    sections
        .iter()
        .enumerate()
        .for_each(|(i, (Section(name, size), diff))| {
            let ratio = size_to_ratio(*size);

            let diff_string = {
                use std::cmp::Ordering::*;
                match diff.cmp(&0) {
                    Equal => "".black(),
                    Greater => format!("+{}", sizeof_fmt(*diff as u32)).yellow(),
                    Less => format!("-{}", sizeof_fmt(-(*diff) as u32)).mint(),
                }
            };

            let uncolored = diff_section_format(
                name.as_str(),
                *size,
                diff_string,
                (ratio * 100.).round() as u8,
            );

            match i % 2 == 0 {
                true => println!("{}", uncolored.s_pink()),
                _ => println!("{}", uncolored.s_purple()),
            }
        });
    println!();
}

fn sizeof_fmt(num: u32) -> String {
    if num < 1024 {
        return format!("{}", num);
    }
    let mut num = num as f32 / 1024.0;
    for unit in ["Ki", "Mi", "Gi", "Ti", "Pi", "Ei", "Zi"] {
        if num.abs() < 1024.0 {
            return format!("{:.2} {}B", num, unit);
        }
        num /= 1024.0
    }
    format!("{:.2} YiB", num)
}
#[cfg(test)]
mod tests {
    #[test]
    fn sizeof_fmt() {
        assert_eq!(crate::display::sizeof_fmt(32), "32");
        assert_eq!(crate::display::sizeof_fmt(128), "128");
        assert_eq!(crate::display::sizeof_fmt(1024), "1.00 KiB");
        assert_eq!(crate::display::sizeof_fmt(1024 * 2), "2.00 KiB");
        assert_eq!(crate::display::sizeof_fmt(1024 * 1024), "1.00 MiB");
    }
}
