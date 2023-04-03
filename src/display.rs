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

const BAR_LENGTH: u8 = 47;

use crate::size::Section;
pub fn print_region(name: &str, region_size: u32, sections: &[(Section, i64)]) {
    let total_usage: u32 = sections.iter().fold(0, |acc, x| acc + x.0 .1);
    let size_to_ratio = |size: u32| (size as f32) / (region_size as f32);
    let percent = (100. * size_to_ratio(total_usage)).round() as u8;
    println!(
        "{}",
        aligned(
            format!("{} used", name).as_str(),
            sizeof_fmt(total_usage).s_purple(),
            "/",
            sizeof_fmt(region_size),
            &percent.to_string(),
            Align::Right
        )
    );

    let bars = sections
        .iter()
        .map(|&(Section(_, size), _)| {
            let ratio = size_to_ratio(size);
            (ratio * BAR_LENGTH as f32).round() as u8
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
        (used_bars..BAR_LENGTH).map(|_| "░").collect::<String>()
    );

    let ratio_to_percent_str = |ratio: f32| ((ratio * 100.).round() as u8).to_string();

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

            let uncolored = aligned(
                name.as_str(),
                sizeof_fmt(*size),
                "",
                diff_string,
                &ratio_to_percent_str(ratio),
                Align::Left,
            );

            match i % 2 == 0 {
                true => println!("{}", uncolored.s_pink()),
                _ => println!("{}", uncolored.s_purple()),
            }
        });
    println!();
}

enum Align {
    Left,
    Right,
}

fn aligned<T, U>(
    title: &str,
    qt1: T,
    separator: &str,
    qt2: U,
    percent: &str,
    align: Align,
) -> String
where
    T: std::fmt::Display,
    U: std::fmt::Display,
{
    match align {
        Align::Left => format!(
            "{}:{:>width$}  {:1}{:<12}{:>7}",
            title,
            qt1,
            separator,
            qt2,
            format!("({}%)", percent),
            width = (24 - title.len())
        ),
        Align::Right => format!(
            "{}:{:>width$}  {:1}{:>12}{:>7}",
            title,
            qt1,
            separator,
            qt2,
            format!("({}%)", percent),
            width = (24 - title.len())
        ),
    }
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
