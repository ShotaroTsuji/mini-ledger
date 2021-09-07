use chrono::NaiveDate;
use nom::branch::alt;
use nom::bytes::complete::{take_while, take_until, take_while1, tag};
use nom::character::complete::{char, digit1, none_of, one_of, space0, space1};
use nom::combinator::{map, opt, recognize};
use nom::multi::many0_count;
use nom::sequence::{preceded, tuple};
use nom::IResult;
use std::borrow::Cow;
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
enum ParseError {
    #[error("Invalid date format")]
    DateFormat,
    #[error("Out-of-range date")]
    DateOutOfRange,
    #[error("Invalid beginning line")]
    BeginningLine,
    #[error("Unclosed code")]
    UnclosedCode,
    #[error("Account is missing")]
    MissingAccount,
    #[error("Duplicate unit")]
    DupUnit,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Cleared,
    Pending,
    Uncleared,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawTransaction<'a> {
    date: RawDate<'a>,
    edate: Option<RawDate<'a>>,
    status: Status,
    code: Option<&'a str>,
    description: &'a str,
    comment: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawAmount<'a> {
    price: &'a str,
    unit: &'a str,
}

impl<'a> RawAmount<'a> {
    pub fn from_str(price: &'a str, unit: &'a str) -> Self {
        Self {
            price: price,
            unit: unit,
        }
    }

    pub fn dollar(price: &'a str) -> Self {
        Self::from_str(price, "$")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawPosting<'a> {
    account: &'a str,
    amount: Option<RawAmount<'a>>,
    assign: Option<RawAmount<'a>>,
    comment: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawDate<'a> {
    pub year: &'a str,
    pub month: &'a str,
    pub day: &'a str,
}

impl<'a> RawDate<'a> {
    pub fn from_ymd(y: &'a str, m: &'a str, d: &'a str) -> Self {
        RawDate {
            year: y,
            month: m,
            day: d,
        }
    }

    pub fn from_triple(t: (&'a str, &'a str, &'a str)) -> Self {
        Self::from_ymd(t.0, t.1, t.2)
    }
}

// Parses a date separated with slashes like `2021/09/07`.
fn date_slash(input: &str) -> IResult<&str, (&str, &str, &str)> {
    map(
        tuple((digit1, char('/'), digit1, char('/'), digit1)),
        |(y, _, m, _, d)| (y, m, d),
    )(input)
}

// Parses a date separated with hyphens like `2021-09-07`.
fn date_dash(input: &str) -> IResult<&str, (&str, &str, &str)> {
    map(
        tuple((digit1, char('-'), digit1, char('-'), digit1)),
        |(y, _, m, _, d)| (y, m, d),
    )(input)
}

/// Parses transaction date
fn date(input: &str) -> IResult<&str, RawDate> {
    map(alt((date_slash, date_dash)), RawDate::from_triple)(input)
}

// Parses transaction status
fn status(input: &str) -> IResult<&str, Status> {
    map(one_of("!*"), |c| match c {
        '*' => Status::Cleared,
        '!' => Status::Pending,
        _ => unreachable!(),
    })(input)
}

// Parses transaction code
//
// A transaction code is a code delimited by parentheses.
fn code(input: &str) -> IResult<&str, &str> {
    map(
        tuple((char('('), take_until(")"), char(')'))),
        |(_, code, _)| code,
    )(input)
}

fn comment(input: &str) -> IResult<&str, &str> {
    preceded(
        tuple((char(';'), space0)),
        take_while(|c| c != '\n')
    )(input)
}

pub fn transaction_header(input: &str) -> IResult<&str, RawTransaction> {
    map(
        tuple((
            date,
            opt(preceded(char('='), date)),
            opt(preceded(space1, status)),
            opt(preceded(space1, code)),
            space1,
            take_while(|c: char| c != ';' && c != '\n'),
            opt(comment),
            opt(char('\n'))
        )),
        |(date, edate, status, code, _, desc, comment, _)| RawTransaction {
            date: date,
            edate: edate,
            status: status.unwrap_or(Status::Uncleared),
            code: code,
            description: desc,
            comment: comment,
        },
    )(input)
}

// Parses an account name
fn account(input: &str) -> IResult<&str, &str> {
    take_while1(|c: char| !c.is_ascii_whitespace())(input)
}

// Parses a decimal value without sign
fn unsigned_decimal(input: &str) -> IResult<&str, &str> {
    recognize(
        tuple((
            digit1,
            many0_count(tuple((char(','), digit1))),
            opt(tuple((char('.'), digit1)))
        ))
    )(input)
}

// Parses a decimal value
fn decimal(input: &str) -> IResult<&str, &str> {
    recognize(tuple((
        opt(one_of("+-")),
        unsigned_decimal,
    )))(input)
}

fn amount_dollar(input: &str) -> IResult<&str, RawAmount> {
    map(
        recognize(
            tuple((
                alt((tag("-$"), tag("$"))),
                unsigned_decimal
            )),
        ),
        RawAmount::dollar
    )(input)
}

fn is_unit_char(c: char) -> bool {
    !c.is_whitespace() &&
        !c.is_ascii_digit() &&
        !".,;:?!-+*/^&|=<>[](){}@".contains(c)
}

/// Parses a commodity unit
/// 
/// TODO: support quoted units
fn unit(input: &str) -> IResult<&str, &str> {
    take_while1(|c: char| is_unit_char(c))(input)
}

/// Parses amount with arbitrary unit like `1000 JPY`.
fn amount_unit(input: &str) -> IResult<&str, RawAmount> {
    map(
        tuple((decimal, opt(preceded(space1, unit)))),
        |(price, unit)| RawAmount::from_str(price, unit.unwrap_or(""))
    )(input)
}

fn assign_amount(input: &str) -> IResult<&str, RawAmount> {
    map(
        tuple((char('='), space0, alt((amount_dollar, amount_unit)))),
        |(_, _, amount)| amount
    )(input)
}

pub fn posting(input: &str) -> IResult<&str, RawPosting> {
    map(
        tuple((
                space1,
                account,
                space0,
                opt(alt((amount_dollar, amount_unit))),
                space0,
                opt(assign_amount),
                space0,
                opt(comment),
                opt(char('\n'))
        )),
        |(_, account, _, amount, _, assign, _, comment, _)| RawPosting {
            account: account,
            amount: amount,
            assign: assign,
            comment: comment,
        }
    )(input)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_date() {
        assert_eq!(
            date("2020/11/30"),
            Ok(("", RawDate::from_ymd("2020", "11", "30")))
        );
        assert_eq!(
            date("2020-01-30"),
            Ok(("", RawDate::from_ymd("2020", "01", "30")))
        );
    }

    #[test]
    fn parse_code() {
        assert_eq!(code("(302)"), Ok(("", "302")));
    }

    #[test]
    fn parse_simple_transaction_header() {
        assert_eq!(
            transaction_header("2020-11-30 * Withdraw\n    "),
            Ok((
                "    ",
                RawTransaction {
                    date: RawDate::from_ymd("2020", "11", "30"),
                    edate: None,
                    status: Status::Cleared,
                    code: None,
                    description: "Withdraw",
                    comment: None,
                }
            ))
        );
        assert_eq!(
            transaction_header("2020-11-30 ! Withdraw   \n"),
            Ok((
                "",
                RawTransaction {
                    date: RawDate::from_ymd("2020", "11", "30"),
                    edate: None,
                    status: Status::Pending,
                    code: None,
                    description: "Withdraw   ",
                    comment: None,
                }
            ))
        );
        assert_eq!(
            transaction_header("2020-11-30 Withdraw ; comment\n"),
            Ok((
                "",
                RawTransaction {
                    date: RawDate::from_ymd("2020", "11", "30"),
                    edate: None,
                    status: Status::Uncleared,
                    code: None,
                    description: "Withdraw ",
                    comment: Some("comment"),
                }
            ))
        );
    }

    #[test]
    fn parse_transaction_with_edate() {
        assert_eq!(
            transaction_header("2020-11-30=2020-12-14 * Withdraw"),
            Ok((
                "",
                RawTransaction {
                    date: RawDate::from_ymd("2020", "11", "30"),
                    edate: Some(RawDate::from_ymd("2020", "12", "14")),
                    status: Status::Cleared,
                    code: None,
                    description: "Withdraw",
                    comment: None,
                }
            ))
        );
    }

    #[test]
    fn parse_transaction_with_code() {
        assert_eq!(
            transaction_header("2020-11-30 * (#100) Withdraw"),
            Ok((
                "",
                RawTransaction {
                    date: RawDate::from_ymd("2020", "11", "30"),
                    edate: None,
                    status: Status::Cleared,
                    code: Some("#100"),
                    description: "Withdraw",
                    comment: None,
                }
            ))
        );
    }

    #[test]
    fn parse_transaction_with_full_options() {
        assert_eq!(
            transaction_header("2020-11-30=2020-12-11 * (#100) Withdraw ; modified\n    Assets"),
            Ok((
                "    Assets",
                RawTransaction {
                    date: RawDate::from_ymd("2020", "11", "30"),
                    edate: Some(RawDate::from_ymd("2020", "12", "11")),
                    status: Status::Cleared,
                    code: Some("#100"),
                    description: "Withdraw ",
                    comment: Some("modified"),
                }
            ))
        );
    }

    #[test]
    fn parse_decimal_values() {
        assert_eq!(decimal("1000"), Ok(("", "1000")));
        assert_eq!(decimal("-9900"), Ok(("", "-9900")));
        assert_eq!(decimal("+10.49"), Ok(("", "+10.49")));
        assert_eq!(decimal("24,000"), Ok(("", "24,000")));
        assert_eq!(decimal("12,345.992"), Ok(("", "12,345.992")));
    }

    #[test]
    fn parse_dollar_amount() {
        assert_eq!(amount_dollar("$100"), Ok(("", RawAmount::dollar("$100"))));
        assert_eq!(amount_dollar("$10.0"), Ok(("", RawAmount::dollar("$10.0"))));
        assert_eq!(amount_dollar("-$5.0"), Ok(("", RawAmount::dollar("-$5.0"))));
    }

    #[test]
    fn parse_plain_amount() {
        assert_eq!(amount_unit("0"), Ok(("", RawAmount::from_str("0", ""))));
        assert_eq!(amount_unit("11.0"), Ok(("", RawAmount::from_str("11.0", ""))));
    }

    #[test]
    fn parse_unit_amount() {
        assert_eq!(
            amount_unit("320 JPY"),
            Ok(("", RawAmount::from_str("320", "JPY")))
        );
        assert_eq!(
            amount_unit("-12.5 JPY"),
            Ok(("", RawAmount::from_str("-12.5", "JPY")))
        );
        assert_eq!(
            amount_unit("1,000 VTI"),
            Ok(("", RawAmount::from_str("1,000", "VTI")))
        );
    }

    #[test]
    fn parse_assign_amount() {
        assert_eq!(
            assign_amount("= 100 JPY"),
            Ok(("", RawAmount::from_str("100", "JPY")))
        );
        assert_eq!(
            assign_amount("= 0"),
            Ok(("", RawAmount::from_str("0", "")))
        );
    }

    #[test]
    fn parse_normal_posting() {
        assert_eq!(
            posting("    Assets:Cash $100.05\n"),
            Ok((
                "",
                RawPosting {
                    account: "Assets:Cash",
                    amount: Some(RawAmount::from_str("$100.05", "$")),
                    assign: None,
                    comment: None,
                }
            ))
        );
        assert_eq!(
            posting("    Assets:Cash 3000 JPY   "),
            Ok((
                "",
                RawPosting {
                    account: "Assets:Cash",
                    amount: Some(RawAmount::from_str("3000", "JPY")),
                    assign: None,
                    comment: None,
                }
            ))
        );
        assert_eq!(
            posting("    Liabilities:CreditCard -3000 JPY ; comment"),
            Ok((
                "",
                RawPosting {
                    account: "Liabilities:CreditCard",
                    amount: Some(RawAmount::from_str("-3000", "JPY")),
                    assign: None,
                    comment: Some("comment"),
                }
            ))
        );
    }

    #[test]
    fn parse_assign_posting() {
        assert_eq!(
            posting("    Assets:Cash    500 JPY = 3000 JPY\n"),
            Ok((
                "",
                RawPosting {
                    account: "Assets:Cash",
                    amount: Some(RawAmount::from_str("500", "JPY")),
                    assign: Some(RawAmount::from_str("3000", "JPY")),
                    comment: None,
                }
            ))
        );
        assert_eq!(
            posting("    Assets:Cash    =0 ; balance the cash\n"),
            Ok((
                "",
                RawPosting {
                    account: "Assets:Cash",
                    amount: None,
                    assign: Some(RawAmount::from_str("0", "")),
                    comment: Some("balance the cash"),
                }
            ))
        );
    }

    #[test]
    fn parse_elided_posting() {
        assert_eq!(
            posting("    Assets:Cash"),
            Ok((
                "",
                RawPosting {
                    account: "Assets:Cash",
                    amount: None,
                    assign: None,
                    comment: None,
                }
            ))
        );
    }

    #[test]
    fn parse_posting_without_indent() {
        assert!(posting("Assets:Cash").is_err());
    }
}
