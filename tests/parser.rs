use mini_ledger::parser::{
    LedgerItem,
    LedgerParser,
};

const LEDGER0: &'static str = r#"
2021-01-01 * 開始残高
    純資産:元入金               10000 JPY
    負債:クレジットカード       -50000 JPY
    資産:普通預金               50000 JPY
    資産:現金                   10000 JPY
"#;

#[test]
fn test_ledger_parser() {
    let mut parser = LedgerParser::new(LEDGER0);

    assert_eq!(parser.next(), Some(LedgerItem::Blank));

    let item = parser.next();
    eprintln!("{:?}", item);
}
