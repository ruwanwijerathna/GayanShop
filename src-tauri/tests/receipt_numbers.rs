//! Short bill numbers (decision #31): `A1234`, a running number that never resets, traced back to its counter.

mod common;

use gayanshop_lib::domain::receipt_no::device_code_sql;

#[tokio::test]
async fn the_counter_code_comes_back_from_either_bill_number_format() {
    let t = common::open().await;
    let cases = [
        ("A1", "T01"),
        ("A1234", "T01"),
        ("B7", "T02"),
        ("I123456", "T09"),
        ("T01-20261108-0042", "T01"),
        ("T03-20261108-0007", "T03"),
    ];
    for (receipt, code) in cases {
        let sql = format!("SELECT {}", device_code_sql(&format!("'{receipt}'")));
        let got: String = sqlx::query_scalar(&sql).fetch_one(t.pool()).await.unwrap();
        assert_eq!(got, code, "{receipt}");
    }
}
