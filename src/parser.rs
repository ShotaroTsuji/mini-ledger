use chrono::NaiveDate;
use nom::branch::alt;
use nom::bytes::complete::{take_while, take_until, take_while1};
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawPosting<'a> {
    account: &'a str,
    amount: Option<RawAmount<'a>>,
    assign: Option<RawAmount<'a>>,
    comment: &'a str,
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

fn account(input: &str) -> IResult<&str, &str> {
    take_while1(|c: char| !c.is_ascii_whitespace())(input)
}

fn decimal(input: &str) -> IResult<&str, &str> {
    recognize(tuple((
        opt(one_of("+-")),
        digit1,
        many0_count(tuple((char(','), digit1))),
        opt(tuple((char('.'), digit1))),
    )))(input)
}

fn amount_dollar(input: &str) -> IResult<&str, RawAmount> {
    let (input, result) = tuple((space1, char('$'), decimal))(input)?;

    Ok((
        input,
        RawAmount {
            price: result.2,
            unit: "$",
        },
    ))
}

fn unit(input: &str) -> IResult<&str, &str> {
    take_while1(|c: char| !c.is_ascii_whitespace())(input)
}

fn amount_unit(input: &str) -> IResult<&str, RawAmount> {
    let (input, result) = tuple((space1, decimal, space1, unit))(input)?;

    Ok((
        input,
        RawAmount {
            price: result.1,
            unit: result.3,
        },
    ))
}

fn assign_amount(input: &str) -> IResult<&str, RawAmount> {
    let (input, _) = space1(input)?;
    let (input, _) = char('=')(input)?;
    let (input, _) = space1(input)?;
    let (input, price) = decimal(input)?;
    let (input, result) = opt(tuple((space1, unit)))(input)?;

    Ok((
        input,
        RawAmount {
            price: price,
            unit: result.map(|x| x.1).unwrap_or(""),
        },
    ))
}

pub fn posting(input: &str) -> IResult<&str, RawPosting> {
    let (input, _) = space1(input)?;
    let (input, account) = account(input)?;
    let (input, amount) = opt(alt((amount_dollar, amount_unit)))(input)?;
    let (input, assign) = opt(assign_amount)(input)?;
    let (input, remain) = opt(take_until(";"))(input)?;

    let comment = match remain {
        Some(_) => input,
        None => "",
    };

    let posting = RawPosting {
        account: account,
        amount: amount,
        assign: assign,
        comment: comment,
    };

    Ok(("", posting))
}

pub fn is_transaction_header(input: &str) -> bool {
    match input.chars().nth(0) {
        Some(c) if c.is_ascii_digit() => true,
        _ => false,
    }
}

pub fn is_posting(input: &str) -> bool {
    let result = tuple::<_, _, (), _>((space1, none_of(";")))(input);
    result.is_ok()
}

pub fn is_posting_comment(input: &str) -> bool {
    let result = tuple::<_, _, (), _>((space1, one_of(";")))(input);
    result.is_ok()
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
    fn decimal_ok() {
        assert_eq!(decimal("1000"), Ok(("", "1000")));
        assert_eq!(decimal("-9900"), Ok(("", "-9900")));
        assert_eq!(decimal("+10.49"), Ok(("", "+10.49")));
        assert_eq!(decimal("24,000"), Ok(("", "24,000")));
        assert_eq!(decimal("12,345.992"), Ok(("", "12,345.992")));
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
    fn trans_header_edate_ok() {
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
    fn trans_header_code_ok() {
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
    fn normal_posting() {
        assert_eq!(
            posting("    Assets:Cash $100.05"),
            Ok((
                "",
                RawPosting {
                    account: "Assets:Cash",
                    amount: Some(RawAmount::from_str("100.05", "$")),
                    assign: None,
                    comment: "",
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
                    comment: "",
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
                    comment: "; comment",
                }
            ))
        );
    }

    #[test]
    fn assign_ok() {
        assert_eq!(
            assign_amount(" = 3000 JPY"),
            Ok(("", RawAmount::from_str("3000", "JPY")))
        );
        assert_eq!(
            assign_amount(" = 0"),
            Ok(("", RawAmount::from_str("0", "")))
        );
    }

    #[test]
    fn assign_posting() {
        assert_eq!(
            posting("    Assets:Cash    500 JPY = 3000 JPY"),
            Ok((
                "",
                RawPosting {
                    account: "Assets:Cash",
                    amount: Some(RawAmount::from_str("500", "JPY")),
                    assign: Some(RawAmount::from_str("3000", "JPY")),
                    comment: "",
                }
            ))
        );
    }

    #[test]
    fn elided_posting() {
        assert_eq!(
            posting("    Assets:Cash"),
            Ok((
                "",
                RawPosting {
                    account: "Assets:Cash",
                    amount: None,
                    assign: None,
                    comment: "",
                }
            ))
        );
    }

    #[test]
    fn predicates() {
        assert_eq!(is_transaction_header("2020-10-05 * Withdraw"), true);
        assert_eq!(is_posting("2020-10-05 * Withdraw"), false);
        assert_eq!(is_posting_comment("2020-10-05 * Withdraw"), false);

        assert_eq!(is_transaction_header("    Liabilities:CreditCard"), false);
        assert_eq!(is_posting("    Liabilities:CreditCard"), true);
        assert_eq!(is_posting_comment("    Liabilities:CreditCard"), false);

        assert_eq!(is_transaction_header("    ; comment"), false);
        assert_eq!(is_posting("    ; comment"), false);
        assert_eq!(is_posting_comment("    ; comment"), true);
    }
}
