// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 Tom F. <tomf@tomtomtech.net> (https://github.com/tomtom215)

use super::*;
use sqlx::postgres::PgPool;

const URL_ENV: &str = "A2A_TEST_POSTGRES_URL";

/// `Debug` names the type and elides the pool. Needs no server: a
/// lazy pool never connects, so this runs in every configuration.
#[tokio::test]
async fn debug_names_the_counter_and_elides_the_pool() {
    let counter = PostgresRateLimitCounter {
        pool: PgPool::connect_lazy("postgres://nobody@127.0.0.1:1/nowhere")
            .expect("a lazy pool needs no server"),
        counted: AtomicU64::new(0),
    };
    let shown = format!("{counter:?}");
    assert!(
        shown.starts_with("PostgresRateLimitCounter"),
        "Debug must name the type, got: {shown}"
    );
    assert!(
        shown.contains(".."),
        "the pool is elided rather than dumped, got: {shown}"
    );
}

/// A scratch database for one test, dropped at the end. Per test and
/// per process, because the sweep deletes every stale row in the
/// table regardless of caller, so two tests sharing a table would
/// each sweep the other's seed.
struct ScratchDb {
    admin_url: String,
    name: String,
}

impl ScratchDb {
    async fn create(tag: &str) -> Self {
        let admin_url = std::env::var(URL_ENV)
            .unwrap_or_else(|_| panic!("{URL_ENV} must be set for the Postgres suite"));
        let name = format!("a2a_rl_counter_{tag}_{}", std::process::id());
        let pool = PgPool::connect(&admin_url)
            .await
            .expect("connect to admin database");
        let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE IF EXISTS {name}"
        )))
        .execute(&pool)
        .await;
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .execute(&pool)
            .await
            .expect("create scratch database");
        pool.close().await;
        Self { admin_url, name }
    }

    fn url(&self) -> String {
        let base = self.admin_url.rsplit_once('/').expect("url has a path").0;
        format!("{base}/{}", self.name)
    }

    async fn drop_db(self) {
        if let Ok(pool) = PgPool::connect(&self.admin_url).await {
            let _ = sqlx::query(sqlx::AssertSqlSafe(format!(
                "DROP DATABASE IF EXISTS \"{}\" WITH (FORCE)",
                self.name
            )))
            .execute(&pool)
            .await;
            pool.close().await;
        }
    }
}

/// A window width so large that the database's window number is 0 for the
/// life of the test. A test that counts under a real width (60 s) and then
/// reads the row back by number straddles a boundary one run in a hundred;
/// this one cannot.
const WIDE: u64 = 1 << 60;

/// The database's window number under [`WIDE`].
const WIDE_NOW: i64 = 0;

/// Far behind [`WIDE_NOW`] minus the one window the sweep keeps.
const STALE_WINDOW: i64 = -10;

/// What the interceptor would pass: its own clock's window. The counter
/// numbers windows by the database's clock and ignores it.
const REPLICA_WINDOW: u64 = 7;

/// [`WIDE`] as the column stores it.
const WIDE_SECS: i64 = 1 << 60;

/// A row from a window long past, which only a sweep removes. It records the
/// width the counts in these tests use, as a row written by `count` does.
async fn seed_stale_row(pool: &PgPool) {
    sqlx::query(
        "INSERT INTO a2a_rate_limit (caller, window_no, request_count, window_secs) \
         VALUES ('stale', $1, 1, $2)",
    )
    .bind(STALE_WINDOW)
    .bind(WIDE_SECS)
    .execute(pool)
    .await
    .expect("seed a stale window");
}

async fn stale_rows(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM a2a_rate_limit WHERE window_no = $1")
        .bind(STALE_WINDOW)
        .fetch_one(pool)
        .await
        .expect("count stale rows")
}

/// The first count of a counter's life does not sweep: `n > 0` is
/// what keeps `0.is_multiple_of(SWEEP_INTERVAL)` — which is true —
/// from firing a table scan on every fresh replica's first request.
/// Kills `>` → `>=`/`==` and `&&` → `||`, all of which sweep at zero.
#[tokio::test]
#[ignore = "needs a live PostgreSQL (see the module docs)"]
async fn the_first_count_does_not_sweep() {
    let db = ScratchDb::create("first").await;
    let counter = PostgresRateLimitCounter::new(&db.url())
        .await
        .expect("counter connects");
    seed_stale_row(&counter.pool).await;

    let seen = counter
        .count("caller", REPLICA_WINDOW, WIDE)
        .await
        .expect("first count");
    assert_eq!(seen, 1, "the first count in a window is 1");
    assert_eq!(
        stale_rows(&counter.pool).await,
        1,
        "the stale window survives the first count: no sweep is due yet"
    );

    db.drop_db().await;
}

/// The sweep runs on the `SWEEP_INTERVAL`-th count and not before,
/// and it removes the stale window while leaving the live one intact.
/// Kills `>` → `<` (never sweeps) and `sweep` → `()` (sweeps nothing),
/// which no number of counts distinguishes from a working sweep unless
/// the table is inspected directly.
#[tokio::test]
#[ignore = "needs a live PostgreSQL (see the module docs)"]
async fn the_sweep_runs_on_the_interval_and_only_clears_past_windows() {
    let db = ScratchDb::create("interval").await;
    let counter = PostgresRateLimitCounter::new(&db.url())
        .await
        .expect("counter connects");
    seed_stale_row(&counter.pool).await;

    // Counts numbered 0 ..= SWEEP_INTERVAL - 1 are all before the
    // first sweep.
    for _ in 0..SWEEP_INTERVAL {
        counter
            .count("caller", REPLICA_WINDOW, WIDE)
            .await
            .expect("count");
    }
    assert_eq!(
        stale_rows(&counter.pool).await,
        1,
        "SWEEP_INTERVAL counts have been made and the sweep has not yet run"
    );

    // Count number SWEEP_INTERVAL is the first multiple above zero.
    let seen = counter
        .count("caller", REPLICA_WINDOW, WIDE)
        .await
        .expect("count");
    assert_eq!(seen, SWEEP_INTERVAL + 1);
    assert_eq!(
        stale_rows(&counter.pool).await,
        0,
        "the sweep on count SWEEP_INTERVAL removes the stale window"
    );
    let live: i64 = sqlx::query_scalar(
        "SELECT request_count FROM a2a_rate_limit WHERE caller = 'caller' AND window_no = $1",
    )
    .bind(WIDE_NOW)
    .fetch_one(&counter.pool)
    .await
    .expect("the live window's row");
    assert_eq!(
        u64::try_from(live).expect("non-negative"),
        SWEEP_INTERVAL + 1,
        "the sweep only clears windows before the current one"
    );

    db.drop_db().await;
}

/// The rows of `caller` as `(window_no, request_count)`.
async fn rows_of(pool: &PgPool, caller: &str) -> Vec<(i64, i64)> {
    sqlx::query_as(
        "SELECT window_no, request_count FROM a2a_rate_limit WHERE caller = $1 \
          ORDER BY window_no",
    )
    .bind(caller)
    .fetch_all(pool)
    .await
    .expect("rows")
}

/// The window number is the database's, whatever the caller passes (E-F16).
/// Three counts under three different caller windows, one of them out of the
/// range of `i64`, land in the one row the database's clock names: a replica
/// whose clock is a second ahead no longer counts in a row of its own.
#[tokio::test]
#[ignore = "needs a live PostgreSQL (see the module docs)"]
async fn the_window_is_the_databases_whatever_the_caller_passes() {
    let db = ScratchDb::create("db_window").await;
    let counter = PostgresRateLimitCounter::new(&db.url())
        .await
        .expect("counter connects");

    for (window, expected) in [(7, 1), (8, 2), (u64::MAX, 3)] {
        let seen = counter.count("k", window, WIDE).await.expect("count");
        assert_eq!(
            seen, expected,
            "the caller's window {window} must not pick the row"
        );
    }
    assert_eq!(rows_of(&counter.pool, "k").await, vec![(WIDE_NOW, 3)]);

    db.drop_db().await;
}

/// The number is `floor(epoch / width)`: read the database's own figure on
/// both sides of the count, so a boundary crossed in between is allowed and
/// nothing else is. Two widths, so that a formula that happens to agree at
/// one (`epoch / 60`) does not pass for the other.
#[tokio::test]
#[ignore = "needs a live PostgreSQL (see the module docs)"]
async fn the_window_number_is_the_epoch_divided_by_the_width() {
    let db = ScratchDb::create("db_formula").await;
    let counter = PostgresRateLimitCounter::new(&db.url())
        .await
        .expect("counter connects");

    for width in [60_i64, 7] {
        let at = || async {
            sqlx::query_scalar::<_, i64>("SELECT floor(extract(epoch from now()) / $1)::bigint")
                .bind(width)
                .fetch_one(&counter.pool)
                .await
                .expect("the database's own window")
        };
        let caller = format!("width-{width}");
        let before = at().await;
        counter
            .count(&caller, 0, u64::try_from(width).expect("positive"))
            .await
            .expect("count");
        let after = at().await;

        let rows = rows_of(&counter.pool, &caller).await;
        assert_eq!(rows.len(), 1, "one count, one row: {rows:?}");
        let (window_no, requests) = rows[0];
        assert!(
            window_no == before || window_no == after,
            "window {window_no} for width {width} is neither {before} nor {after}"
        );
        assert_eq!(requests, 1);
    }

    db.drop_db().await;
}

/// The sweep keeps the window before the current one and drops what is older,
/// on the database's clock. Rows seeded around [`WIDE_NOW`] (the
/// database's window, 0): `-2` goes, `-1` stays (the previous window), and `1`
/// stays (a window ahead of the database, which a sweep must not read as
/// "past" however far ahead the replica that wrote it was).
#[tokio::test]
#[ignore = "needs a live PostgreSQL (see the module docs)"]
async fn the_sweep_keeps_the_previous_window_and_drops_older_ones() {
    let db = ScratchDb::create("sweep_grace").await;
    let counter = PostgresRateLimitCounter::new(&db.url())
        .await
        .expect("counter connects");
    for window in [-2_i64, -1, 1] {
        sqlx::query(
            "INSERT INTO a2a_rate_limit (caller, window_no, request_count, window_secs) \
             VALUES ('seed', $1, 1, $2)",
        )
        .bind(window)
        .bind(WIDE_SECS)
        .execute(&counter.pool)
        .await
        .expect("seed");
    }

    for _ in 0..=SWEEP_INTERVAL {
        counter
            .count("caller", REPLICA_WINDOW, WIDE)
            .await
            .expect("count");
    }

    let kept: Vec<i64> = rows_of(&counter.pool, "seed")
        .await
        .into_iter()
        .map(|(window, _)| window)
        .collect();
    assert_eq!(kept, vec![-1, 1]);

    db.drop_db().await;
}

/// A width that cannot be a number of seconds, or is zero, is an error and
/// not a count: the interceptor reads an error as "the shared counter is
/// unavailable" and counts locally, where a made-up total would admit or
/// refuse traffic on no evidence.
#[tokio::test]
#[ignore = "needs a live PostgreSQL (see the module docs)"]
async fn an_unusable_width_is_an_error_not_a_count() {
    let db = ScratchDb::create("bad_width").await;
    let counter = PostgresRateLimitCounter::new(&db.url())
        .await
        .expect("counter connects");

    let too_wide = counter.count("k", 0, u64::MAX).await.expect_err("too wide");
    assert!(
        too_wide.to_string().contains("out of range"),
        "got: {too_wide}"
    );
    let zero = counter.count("k", 0, 0).await.expect_err("zero");
    assert!(
        zero.to_string().contains("division by zero"),
        "the database refuses it, and says why: {zero}"
    );
    assert_eq!(rows_of(&counter.pool, "k").await, Vec::<(i64, i64)>::new());

    db.drop_db().await;
}

/// The width a row was counted under, by caller.
async fn width_of(pool: &PgPool, caller: &str) -> Vec<i64> {
    sqlx::query_scalar(
        "SELECT window_secs FROM a2a_rate_limit WHERE caller = $1 ORDER BY window_no",
    )
    .bind(caller)
    .fetch_all(pool)
    .await
    .expect("widths")
}

/// A sweep of one width leaves the rows of another alone. The rate limiter
/// and the authentication-failure throttle share this table and count in
/// windows of different widths; window numbers of different widths are not
/// comparable, and before the width was recorded a 60 s sweep read a 300 s
/// row's number as long past and deleted a live count (measured: a row of
/// count 3). Here a row of width [`WIDE`] holds the database's current
/// window 0, and a sweep of 60 s would put the cut-off at millions.
#[tokio::test]
#[ignore = "needs a live PostgreSQL (see the module docs)"]
async fn a_sweep_leaves_the_rows_of_another_width() {
    let db = ScratchDb::create("other_width").await;
    let counter = PostgresRateLimitCounter::new(&db.url())
        .await
        .expect("counter connects");
    for expected in 1..=3 {
        let seen = counter.count("wide", 0, WIDE).await.expect("count");
        assert_eq!(seen, expected);
    }

    counter.sweep(60).await;

    assert_eq!(
        rows_of(&counter.pool, "wide").await,
        vec![(WIDE_NOW, 3)],
        "a 60 s sweep must not read a row of another width as expired"
    );

    db.drop_db().await;
}

/// The other half: a sweep still removes what is stale for its own width,
/// and a row with no recorded width (written before the column existed, so
/// the default `0`) is removed by any sweep, since nothing says whose it is.
#[tokio::test]
#[ignore = "needs a live PostgreSQL (see the module docs)"]
async fn a_sweep_removes_stale_rows_of_its_width_and_rows_with_none() {
    let db = ScratchDb::create("own_width").await;
    let counter = PostgresRateLimitCounter::new(&db.url())
        .await
        .expect("counter connects");
    for (caller, width) in [("own", 60_i64), ("other", 300), ("legacy", 0)] {
        sqlx::query(
            "INSERT INTO a2a_rate_limit (caller, window_no, request_count, window_secs) \
             VALUES ($1, 1, 4, $2)",
        )
        .bind(caller)
        .bind(width)
        .execute(&counter.pool)
        .await
        .expect("seed");
    }

    counter.sweep(60).await;

    assert!(
        rows_of(&counter.pool, "own").await.is_empty(),
        "stale, same width"
    );
    assert!(
        rows_of(&counter.pool, "legacy").await.is_empty(),
        "no width recorded"
    );
    assert_eq!(
        rows_of(&counter.pool, "other").await,
        vec![(1, 4)],
        "another width's row is the other sweep's to judge"
    );

    db.drop_db().await;
}

/// A count records the width it was numbered under, including onto a row an
/// earlier release wrote (width `0`), which would otherwise be swept as
/// unattributable while it is being counted into.
#[tokio::test]
#[ignore = "needs a live PostgreSQL (see the module docs)"]
async fn a_count_records_its_width_even_on_a_row_without_one() {
    let db = ScratchDb::create("record_width").await;
    let counter = PostgresRateLimitCounter::new(&db.url())
        .await
        .expect("counter connects");
    sqlx::query(
        "INSERT INTO a2a_rate_limit (caller, window_no, request_count, window_secs) \
         VALUES ('legacy', $1, 4, 0)",
    )
    .bind(WIDE_NOW)
    .execute(&counter.pool)
    .await
    .expect("seed a row that records no width");

    assert_eq!(counter.count("fresh", 0, WIDE).await.expect("count"), 1);
    assert_eq!(counter.count("legacy", 0, WIDE).await.expect("count"), 5);

    assert_eq!(width_of(&counter.pool, "fresh").await, vec![WIDE_SECS]);
    assert_eq!(width_of(&counter.pool, "legacy").await, vec![WIDE_SECS]);

    db.drop_db().await;
}

/// A table an earlier release made has no `window_secs`; starting against it
/// adds the column, keeps its rows, and can be done again.
#[tokio::test]
#[ignore = "needs a live PostgreSQL (see the module docs)"]
async fn a_table_from_an_earlier_release_gains_the_width_column() {
    let db = ScratchDb::create("upgrade").await;
    let pool = PgPool::connect(&db.url()).await.expect("connect");
    sqlx::query(
        "CREATE TABLE a2a_rate_limit (caller TEXT NOT NULL, window_no BIGINT NOT NULL, \
         request_count BIGINT NOT NULL, PRIMARY KEY (caller, window_no))",
    )
    .execute(&pool)
    .await
    .expect("the earlier release's table");
    sqlx::query("INSERT INTO a2a_rate_limit VALUES ('old', 7, 2)")
        .execute(&pool)
        .await
        .expect("an earlier row");

    for expected in 1..=2 {
        let counter = PostgresRateLimitCounter::from_pool(pool.clone())
            .await
            .expect("starts against the old table, and again against the new one");
        assert_eq!(
            counter.count("new", 0, WIDE).await.expect("count"),
            expected
        );
    }

    assert_eq!(rows_of(&pool, "old").await, vec![(7, 2)]);
    assert_eq!(width_of(&pool, "old").await, vec![0], "recorded as unknown");
    assert_eq!(width_of(&pool, "new").await, vec![WIDE_SECS]);

    pool.close().await;
    db.drop_db().await;
}
