/// Converts an offset in days from 1970-01-01 to a Gregorian year, month, and day.
pub fn civil_date(days: i64) -> (i64, u8, u8) {
    let shifted = days.rem_euclid(146_097) + 719_468;
    let era = days.div_euclid(146_097) + shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month as u8, day as u8)
}

#[cfg(test)]
mod tests {
    use super::civil_date;

    #[test]
    fn gregorian_dates_across_the_epoch() {
        assert_eq!(civil_date(-25_508), (1900, 3, 1));
        assert_eq!(civil_date(-1), (1969, 12, 31));
        assert_eq!(civil_date(0), (1970, 1, 1));
        assert_eq!(civil_date(11_016), (2000, 2, 29));
    }
}
