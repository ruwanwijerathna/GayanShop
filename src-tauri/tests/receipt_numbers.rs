//! Bill numbers (decision #31): `T01-261108004`, a daily number that restarts each business day, and the two
//! formats (this one and the older `T01-20261108-0042`) read the same way by the SQL that counts bills.

mod common;

use gayanshop_lib::domain::receipt_no::seq_sql;

async fn number(t: &common::TestDb, expr: String) -> i64 {
    sqlx::query_scalar(&format!("SELECT {expr}")).fetch_one(t.pool()).await.unwrap()
}

#[tokio::test]
async fn the_daily_number_comes_back_from_either_bill_number_format() {
    let t = common::open().await;
    let cases = [
        ("T01-261108004", 4),
        ("T02-2611081000", 1000),
        ("T03-261108999", 999),
        ("T01-20261108-0042", 42),
        ("T02-20261108-12345", 12_345),
    ];
    for (receipt, seq) in cases {
        let quoted = format!("'{receipt}'");
        assert_eq!(number(&t, seq_sql(&quoted)).await, seq, "{receipt}");
    }
}
