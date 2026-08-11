use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow};
use sqlx::{Column, Row as SqlxRow, SqlitePool, TypeInfo};
use std::cell::Cell;
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::runtime::{Builder, Runtime};

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub enum Error {
    SqliteFailure((), Option<String>),
    QueryReturnedNoRows,
    InvalidParameterName(String),
    ToSqlConversionFailure(Box<dyn std::error::Error + Send + Sync>),
    InvalidColumnIndex(usize),
    FromSql(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SqliteFailure(_, Some(message)) => f.write_str(message),
            Self::SqliteFailure(_, None) => f.write_str("sqlite failure"),
            Self::QueryReturnedNoRows => f.write_str("query returned no rows"),
            Self::InvalidParameterName(name) => write!(f, "invalid parameter name: {name}"),
            Self::ToSqlConversionFailure(err) => write!(f, "to-sql conversion failed: {err}"),
            Self::InvalidColumnIndex(index) => write!(f, "invalid column index: {index}"),
            Self::FromSql(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

impl From<sqlx::Error> for Error {
    fn from(err: sqlx::Error) -> Self {
        match err {
            sqlx::Error::RowNotFound => Self::QueryReturnedNoRows,
            other => Self::SqliteFailure((), Some(other.to_string())),
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::SqliteFailure((), Some(err.to_string()))
    }
}

#[derive(Debug, Clone)]
pub struct Connection {
    rt: Arc<Runtime>,
    pool: SqlitePool,
    path: Option<PathBuf>,
}

impl Connection {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let rt = sqlite_runtime()?;
        let options = sqlite_options_for_path(&path)?;
        let pool = block_on_runtime(&rt, async {
            SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(options)
                .await
        })?;
        Ok(Self {
            rt,
            pool,
            path: Some(path),
        })
    }

    pub fn open_read_only<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let rt = sqlite_runtime()?;
        let options = SqliteConnectOptions::new().filename(&path).read_only(true);
        let pool = block_on_runtime(&rt, async {
            SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(options)
                .await
        })?;
        Ok(Self {
            rt,
            pool,
            path: Some(path),
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        let rt = sqlite_runtime()?;
        let pool = block_on_runtime(&rt, async {
            SqlitePoolOptions::new()
                .max_connections(1)
                .connect_with(
                    SqliteConnectOptions::from_str("sqlite::memory:")?
                        .create_if_missing(true)
                        .journal_mode(SqliteJournalMode::Memory),
                )
                .await
        })?;
        Ok(Self {
            rt,
            pool,
            path: None,
        })
    }

    pub fn busy_timeout(&self, timeout: Duration) -> Result<()> {
        self.execute_batch(&format!("PRAGMA busy_timeout = {};", timeout.as_millis()))
    }

    pub fn prepare<'c>(&'c self, sql: &str) -> Result<Statement<'c>> {
        Ok(Statement {
            conn: self,
            sql: sql.to_string(),
        })
    }

    pub fn execute<P: Params>(&self, sql: &str, params: P) -> Result<usize> {
        self.block_on(execute_on_pool(
            &self.pool,
            sql,
            Params::into_params(params),
        ))
    }

    pub fn execute_batch(&self, sql: &str) -> Result<()> {
        self.block_on(async {
            for statement in split_sql_batch(sql) {
                if !statement.trim().is_empty() {
                    sqlx::query(statement).execute(&self.pool).await?;
                }
            }
            Ok(())
        })
    }

    pub fn query_row<P, F, T>(&self, sql: &str, params: P, f: F) -> Result<T>
    where
        P: Params,
        F: FnOnce(&Row<'_>) -> Result<T>,
    {
        self.prepare(sql)?.query_row(params, f)
    }

    pub fn transaction(&self) -> Result<Transaction<'_>> {
        self.unchecked_transaction()
    }

    pub fn unchecked_transaction(&self) -> Result<Transaction<'_>> {
        self.execute_batch("BEGIN IMMEDIATE")?;
        Ok(Transaction {
            conn: self,
            active: Cell::new(true),
            last_insert_rowid: Cell::new(0),
        })
    }

    pub fn last_insert_rowid(&self) -> i64 {
        self.block_on(async {
            sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
                .fetch_one(&self.pool)
                .await
                .unwrap_or(0)
        })
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    fn block_on<F>(&self, future: F) -> F::Output
    where
        F: Future + Send,
        F::Output: Send,
    {
        block_on_runtime(&self.rt, future)
    }
}

static SQLITE_RUNTIME: OnceLock<Arc<Runtime>> = OnceLock::new();

fn sqlite_runtime() -> Result<Arc<Runtime>> {
    if let Some(runtime) = SQLITE_RUNTIME.get() {
        return Ok(Arc::clone(runtime));
    }
    let runtime = Arc::new(
        Builder::new_multi_thread()
            .enable_all()
            .build()
            .map_err(Error::from)?,
    );
    let _ = SQLITE_RUNTIME.set(Arc::clone(&runtime));
    Ok(SQLITE_RUNTIME.get().map(Arc::clone).unwrap_or(runtime))
}

fn block_on_runtime<F>(rt: &Runtime, future: F) -> F::Output
where
    F: Future + Send,
    F::Output: Send,
{
    if tokio::runtime::Handle::try_current().is_ok() {
        std::thread::scope(|scope| {
            scope
                .spawn(|| rt.block_on(future))
                .join()
                .expect("sqlite runtime helper thread panicked")
        })
    } else {
        rt.block_on(future)
    }
}

fn sqlite_options_for_path(path: &Path) -> Result<SqliteConnectOptions> {
    Ok(SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal))
}

#[derive(Debug)]
pub struct Statement<'c> {
    conn: &'c Connection,
    sql: String,
}

impl<'c> Statement<'c> {
    pub fn query<P: Params>(&mut self, params: P) -> Result<Rows<'_>> {
        let rows = self.conn.block_on(fetch_rows_on_pool(
            &self.conn.pool,
            &self.sql,
            Params::into_params(params),
        ))?;
        Ok(Rows {
            rows,
            index: 0,
            _marker: std::marker::PhantomData,
        })
    }

    pub fn query_map<P, F, T>(&mut self, params: P, mut f: F) -> Result<MappedRows<T>>
    where
        P: Params,
        F: FnMut(&Row<'_>) -> Result<T>,
    {
        let rows = self.query(params)?;
        let mut mapped = Vec::new();
        for row in rows.rows.iter() {
            mapped.push(f(row)?);
        }
        Ok(MappedRows {
            inner: mapped.into_iter().map(Ok).collect::<Vec<_>>().into_iter(),
        })
    }

    pub fn query_row<P, F, T>(&mut self, params: P, f: F) -> Result<T>
    where
        P: Params,
        F: FnOnce(&Row<'_>) -> Result<T>,
    {
        let mut rows = self.query(params)?;
        match rows.next()? {
            Some(row) => f(row),
            None => Err(Error::QueryReturnedNoRows),
        }
    }

    pub fn execute<P: Params>(&mut self, params: P) -> Result<usize> {
        self.conn.block_on(execute_on_pool(
            &self.conn.pool,
            &self.sql,
            Params::into_params(params),
        ))
    }
}

#[derive(Debug)]
pub struct Rows<'stmt> {
    rows: Vec<Row<'static>>,
    index: usize,
    _marker: std::marker::PhantomData<&'stmt ()>,
}

impl Rows<'_> {
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Result<Option<&Row<'_>>> {
        if self.index >= self.rows.len() {
            return Ok(None);
        }
        let row = &self.rows[self.index];
        self.index += 1;
        Ok(Some(row))
    }
}

pub struct MappedRows<T> {
    inner: std::vec::IntoIter<Result<T>>,
}

impl<T> Iterator for MappedRows<T> {
    type Item = Result<T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }
}

#[derive(Debug, Clone)]
pub struct Row<'r> {
    values: Vec<types::Value>,
    _marker: std::marker::PhantomData<&'r ()>,
}

impl<'r> Row<'r> {
    pub fn get<I, T>(&self, index: I) -> Result<T>
    where
        I: RowIndex,
        T: FromValue,
    {
        let index = index.index(self)?;
        let value = self
            .values
            .get(index)
            .ok_or(Error::InvalidColumnIndex(index))?;
        T::from_value(value)
    }
}

pub trait RowIndex {
    fn index(self, row: &Row<'_>) -> Result<usize>;
}

impl RowIndex for usize {
    fn index(self, _row: &Row<'_>) -> Result<usize> {
        Ok(self)
    }
}

impl RowIndex for i32 {
    fn index(self, _row: &Row<'_>) -> Result<usize> {
        usize::try_from(self).map_err(|_| Error::InvalidColumnIndex(0))
    }
}

pub mod types {
    #[derive(Debug, Clone, PartialEq)]
    pub enum Value {
        Null,
        Integer(i64),
        Real(f64),
        Text(String),
        Blob(Vec<u8>),
    }
}

pub trait FromValue: Sized {
    fn from_value(value: &types::Value) -> Result<Self>;
}

impl FromValue for String {
    fn from_value(value: &types::Value) -> Result<Self> {
        match value {
            types::Value::Text(value) => Ok(value.clone()),
            types::Value::Integer(value) => Ok(value.to_string()),
            types::Value::Real(value) => Ok(value.to_string()),
            types::Value::Null => Err(Error::FromSql("cannot read NULL as String".to_string())),
            types::Value::Blob(_) => Err(Error::FromSql("cannot read BLOB as String".to_string())),
        }
    }
}

impl FromValue for i64 {
    fn from_value(value: &types::Value) -> Result<Self> {
        match value {
            types::Value::Integer(value) => Ok(*value),
            types::Value::Real(value) => Ok(*value as i64),
            types::Value::Text(value) => value
                .parse()
                .map_err(|err| Error::FromSql(format!("cannot parse i64: {err}"))),
            types::Value::Null => Err(Error::FromSql("cannot read NULL as i64".to_string())),
            types::Value::Blob(_) => Err(Error::FromSql("cannot read BLOB as i64".to_string())),
        }
    }
}

impl FromValue for i32 {
    fn from_value(value: &types::Value) -> Result<Self> {
        i64::from_value(value)?
            .try_into()
            .map_err(|err| Error::FromSql(format!("cannot convert i64 to i32: {err}")))
    }
}

impl FromValue for usize {
    fn from_value(value: &types::Value) -> Result<Self> {
        i64::from_value(value)?
            .try_into()
            .map_err(|err| Error::FromSql(format!("cannot convert i64 to usize: {err}")))
    }
}

impl FromValue for f64 {
    fn from_value(value: &types::Value) -> Result<Self> {
        match value {
            types::Value::Real(value) => Ok(*value),
            types::Value::Integer(value) => Ok(*value as f64),
            types::Value::Text(value) => value
                .parse()
                .map_err(|err| Error::FromSql(format!("cannot parse f64: {err}"))),
            types::Value::Null => Err(Error::FromSql("cannot read NULL as f64".to_string())),
            types::Value::Blob(_) => Err(Error::FromSql("cannot read BLOB as f64".to_string())),
        }
    }
}

impl FromValue for bool {
    fn from_value(value: &types::Value) -> Result<Self> {
        Ok(i64::from_value(value)? != 0)
    }
}

impl<T: FromValue> FromValue for Option<T> {
    fn from_value(value: &types::Value) -> Result<Self> {
        match value {
            types::Value::Null => Ok(None),
            other => T::from_value(other).map(Some),
        }
    }
}

impl FromValue for Vec<u8> {
    fn from_value(value: &types::Value) -> Result<Self> {
        match value {
            types::Value::Blob(value) => Ok(value.clone()),
            types::Value::Null => Err(Error::FromSql("cannot read NULL as BLOB".to_string())),
            _ => Err(Error::FromSql(
                "cannot read non-BLOB value as BLOB".to_string(),
            )),
        }
    }
}

pub trait ToValue {
    fn to_value(self) -> types::Value;
}

impl ToValue for types::Value {
    fn to_value(self) -> types::Value {
        self
    }
}

impl ToValue for &types::Value {
    fn to_value(self) -> types::Value {
        self.clone()
    }
}

impl ToValue for String {
    fn to_value(self) -> types::Value {
        types::Value::Text(self)
    }
}

impl ToValue for &String {
    fn to_value(self) -> types::Value {
        types::Value::Text(self.clone())
    }
}

impl ToValue for &str {
    fn to_value(self) -> types::Value {
        types::Value::Text(self.to_string())
    }
}

impl ToValue for i64 {
    fn to_value(self) -> types::Value {
        types::Value::Integer(self)
    }
}

impl ToValue for &i64 {
    fn to_value(self) -> types::Value {
        types::Value::Integer(*self)
    }
}

impl ToValue for i32 {
    fn to_value(self) -> types::Value {
        types::Value::Integer(self as i64)
    }
}

impl ToValue for usize {
    fn to_value(self) -> types::Value {
        types::Value::Integer(self as i64)
    }
}

impl ToValue for f64 {
    fn to_value(self) -> types::Value {
        types::Value::Real(self)
    }
}

impl ToValue for &f64 {
    fn to_value(self) -> types::Value {
        types::Value::Real(*self)
    }
}

impl ToValue for bool {
    fn to_value(self) -> types::Value {
        types::Value::Integer(i64::from(self))
    }
}

impl ToValue for () {
    fn to_value(self) -> types::Value {
        types::Value::Null
    }
}

impl ToValue for &i32 {
    fn to_value(self) -> types::Value {
        types::Value::Integer(*self as i64)
    }
}

impl ToValue for &usize {
    fn to_value(self) -> types::Value {
        types::Value::Integer(*self as i64)
    }
}

impl ToValue for &bool {
    fn to_value(self) -> types::Value {
        types::Value::Integer(i64::from(*self))
    }
}

impl ToValue for &&str {
    fn to_value(self) -> types::Value {
        types::Value::Text((*self).to_string())
    }
}

impl ToValue for &&String {
    fn to_value(self) -> types::Value {
        types::Value::Text((*self).clone())
    }
}

impl<T: ToValue> ToValue for Option<T> {
    fn to_value(self) -> types::Value {
        self.map(ToValue::to_value).unwrap_or(types::Value::Null)
    }
}

impl<T: ToValue + Clone> ToValue for &Option<T> {
    fn to_value(self) -> types::Value {
        self.clone().to_value()
    }
}

impl<T: ToValue + Clone> ToValue for &&Option<T> {
    fn to_value(self) -> types::Value {
        (*self).clone().to_value()
    }
}

impl ToValue for Vec<u8> {
    fn to_value(self) -> types::Value {
        types::Value::Blob(self)
    }
}

impl ToValue for &Vec<u8> {
    fn to_value(self) -> types::Value {
        types::Value::Blob(self.clone())
    }
}

impl ToValue for &&[u8] {
    fn to_value(self) -> types::Value {
        types::Value::Blob((*self).to_vec())
    }
}

impl ToValue for &[u8] {
    fn to_value(self) -> types::Value {
        types::Value::Blob(self.to_vec())
    }
}

pub trait IntoParams {
    fn into_params(self) -> Vec<types::Value>;
}

pub trait Params {
    fn into_params(self) -> Vec<types::Value>;
}

impl<T: IntoParams> Params for T {
    fn into_params(self) -> Vec<types::Value> {
        IntoParams::into_params(self)
    }
}

impl IntoParams for Vec<types::Value> {
    fn into_params(self) -> Vec<types::Value> {
        self
    }
}

impl IntoParams for &[types::Value] {
    fn into_params(self) -> Vec<types::Value> {
        self.to_vec()
    }
}

impl IntoParams for &Vec<types::Value> {
    fn into_params(self) -> Vec<types::Value> {
        self.clone()
    }
}

impl IntoParams for () {
    fn into_params(self) -> Vec<types::Value> {
        Vec::new()
    }
}

impl IntoParams for [(); 0] {
    fn into_params(self) -> Vec<types::Value> {
        Vec::new()
    }
}

macro_rules! array_params {
    ($($n:expr),+ $(,)?) => {
        $(
            impl<T: ToValue> IntoParams for [T; $n] {
                fn into_params(self) -> Vec<types::Value> {
                    self.into_iter().map(ToValue::to_value).collect()
                }
            }
        )+
    };
}

array_params!(1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12);

macro_rules! tuple_params {
    ($($name:ident),+ $(,)?) => {
        impl<$($name: ToValue),+> IntoParams for ($($name,)+) {
            #[allow(non_snake_case)]
            fn into_params(self) -> Vec<types::Value> {
                let ($($name,)+) = self;
                vec![$($name.to_value(),)+]
            }
        }
    };
}

tuple_params!(A);
tuple_params!(A, B);
tuple_params!(A, B, C);
tuple_params!(A, B, C, D);
tuple_params!(A, B, C, D, E);
tuple_params!(A, B, C, D, E, F);
tuple_params!(A, B, C, D, E, F, G);
tuple_params!(A, B, C, D, E, F, G, H);
tuple_params!(A, B, C, D, E, F, G, H, I);
tuple_params!(A, B, C, D, E, F, G, H, I, J);
tuple_params!(A, B, C, D, E, F, G, H, I, J, K);
tuple_params!(A, B, C, D, E, F, G, H, I, J, K, L);
tuple_params!(A, B, C, D, E, F, G, H, I, J, K, L, M);
tuple_params!(A, B, C, D, E, F, G, H, I, J, K, L, M, N);
tuple_params!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O);
tuple_params!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P);
tuple_params!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q);
tuple_params!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R);
tuple_params!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S);
tuple_params!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T);
tuple_params!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U);
tuple_params!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V);
tuple_params!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W);
tuple_params!(A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X);

pub fn params_from_iter<I, T>(iter: I) -> Vec<types::Value>
where
    I: IntoIterator<Item = T>,
    T: ToValue,
{
    iter.into_iter().map(ToValue::to_value).collect()
}

#[macro_export]
macro_rules! params {
    ($($value:expr),* $(,)?) => {
        vec![$($crate::ToValue::to_value(&$value)),*]
    };
}

pub trait OptionalExtension<T> {
    fn optional(self) -> Result<Option<T>>;
}

impl<T> OptionalExtension<T> for Result<T> {
    fn optional(self) -> Result<Option<T>> {
        match self {
            Ok(value) => Ok(Some(value)),
            Err(Error::QueryReturnedNoRows) => Ok(None),
            Err(err) => Err(err),
        }
    }
}

#[derive(Debug)]
pub struct Transaction<'c> {
    conn: &'c Connection,
    active: Cell<bool>,
    last_insert_rowid: Cell<i64>,
}

impl<'c> Transaction<'c> {
    pub fn execute<P: Params>(&self, sql: &str, params: P) -> Result<usize> {
        let affected = self.conn.execute(sql, params)?;
        let rowid = self.conn.last_insert_rowid();
        self.last_insert_rowid.set(rowid);
        Ok(affected)
    }

    pub fn execute_batch(&self, sql: &str) -> Result<()> {
        self.conn.execute_batch(sql)
    }

    pub fn query_row<P, F, T>(&self, sql: &str, params: P, f: F) -> Result<T>
    where
        P: Params,
        F: FnOnce(&Row<'_>) -> Result<T>,
    {
        self.conn.query_row(sql, params, f)
    }

    pub fn prepare<'t>(&'t self, sql: &str) -> Result<TransactionStatement<'t, 'c>> {
        Ok(TransactionStatement {
            tx: self,
            sql: sql.to_string(),
        })
    }

    pub fn last_insert_rowid(&self) -> i64 {
        self.last_insert_rowid.get()
    }

    pub fn commit(self) -> Result<()> {
        if self.active.replace(false) {
            self.conn.execute_batch("COMMIT")?;
        }
        Ok(())
    }
}

impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if self.active.replace(false) {
            let _ = self.conn.execute_batch("ROLLBACK");
        }
    }
}

pub struct TransactionStatement<'t, 'c> {
    tx: &'t Transaction<'c>,
    sql: String,
}

impl TransactionStatement<'_, '_> {
    pub fn execute<P: Params>(&self, params: P) -> Result<usize> {
        self.tx.execute(&self.sql, params)
    }
}

pub mod backup {
    use super::{Connection, Result};
    use std::fs;
    use std::time::Duration;

    pub struct Backup<'a, 'b> {
        source: &'a Connection,
        target: &'b mut Connection,
    }

    impl<'a, 'b> Backup<'a, 'b> {
        pub fn new(source: &'a Connection, target: &'b mut Connection) -> Result<Self> {
            Ok(Self { source, target })
        }

        pub fn run_to_completion(
            &self,
            _pages_per_step: i32,
            _sleep: Duration,
            _progress: Option<fn(i32)>,
        ) -> Result<()> {
            let Some(source_path) = self.source.path() else {
                return Err(super::Error::SqliteFailure(
                    (),
                    Some("source sqlite connection has no backing file".to_string()),
                ));
            };
            let Some(target_path) = self.target.path() else {
                return Err(super::Error::SqliteFailure(
                    (),
                    Some("target sqlite connection has no backing file".to_string()),
                ));
            };
            self.source.execute_batch("PRAGMA wal_checkpoint(FULL);")?;
            fs::copy(source_path, target_path)?;
            Ok(())
        }
    }
}

async fn execute_on_pool(pool: &SqlitePool, sql: &str, params: Vec<types::Value>) -> Result<usize> {
    let (sql, params) = normalize_sql_and_params(sql, params)?;
    let result = bind_values(sqlx::query(&sql), &params)
        .execute(pool)
        .await?;
    Ok(result.rows_affected() as usize)
}

async fn fetch_rows_on_pool(
    pool: &SqlitePool,
    sql: &str,
    params: Vec<types::Value>,
) -> Result<Vec<Row<'static>>> {
    let (sql, params) = normalize_sql_and_params(sql, params)?;
    let rows = bind_values(sqlx::query(&sql), &params)
        .fetch_all(pool)
        .await?;
    rows.into_iter().map(row_from_sqlx).collect()
}

fn normalize_sql_and_params(
    sql: &str,
    params: Vec<types::Value>,
) -> Result<(String, Vec<types::Value>)> {
    let mut out = String::with_capacity(sql.len());
    let mut ordered = Vec::new();
    let mut max_index = 0usize;
    let bytes = sql.as_bytes();
    let mut index = 0usize;

    while index < bytes.len() {
        if bytes[index] == b'?' {
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            let param_index = if end > start {
                let explicit = sql[start..end].parse::<usize>().map_err(|err| {
                    Error::SqliteFailure((), Some(format!("invalid sqlite parameter index: {err}")))
                })?;
                max_index = max_index.max(explicit);
                explicit
            } else {
                max_index += 1;
                max_index
            };
            let value = params
                .get(param_index.saturating_sub(1))
                .ok_or_else(|| Error::InvalidParameterName(format!("?{param_index}")))?
                .clone();
            ordered.push(value);
            out.push('?');
            index = end;
            continue;
        }

        out.push(bytes[index] as char);
        index += 1;
    }

    Ok((out, ordered))
}

fn bind_values<'q>(
    mut query: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    params: &'q [types::Value],
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    for value in params {
        query = match value {
            types::Value::Null => query.bind(Option::<i64>::None),
            types::Value::Integer(value) => query.bind(*value),
            types::Value::Real(value) => query.bind(*value),
            types::Value::Text(value) => query.bind(value),
            types::Value::Blob(value) => query.bind(value),
        };
    }
    query
}

fn row_from_sqlx(row: SqliteRow) -> Result<Row<'static>> {
    let mut values = Vec::new();
    for (index, column) in row.columns().iter().enumerate() {
        let type_name = column.type_info().name().to_ascii_uppercase();
        let value = if type_name.contains("INT") {
            row.try_get::<Option<i64>, _>(index).map(|value| {
                value
                    .map(types::Value::Integer)
                    .unwrap_or(types::Value::Null)
            })?
        } else if type_name.contains("REAL")
            || type_name.contains("FLOA")
            || type_name.contains("DOUB")
        {
            row.try_get::<Option<f64>, _>(index)
                .map(|value| value.map(types::Value::Real).unwrap_or(types::Value::Null))?
        } else if type_name.contains("BLOB") {
            row.try_get::<Option<Vec<u8>>, _>(index)
                .map(|value| value.map(types::Value::Blob).unwrap_or(types::Value::Null))?
        } else {
            match row.try_get::<Option<String>, _>(index) {
                Ok(value) => value.map(types::Value::Text).unwrap_or(types::Value::Null),
                Err(_) => match row.try_get::<Option<i64>, _>(index) {
                    Ok(value) => value
                        .map(types::Value::Integer)
                        .unwrap_or(types::Value::Null),
                    Err(_) => match row.try_get::<Option<f64>, _>(index) {
                        Ok(value) => value.map(types::Value::Real).unwrap_or(types::Value::Null),
                        Err(_) => row.try_get::<Option<Vec<u8>>, _>(index).map(|value| {
                            value.map(types::Value::Blob).unwrap_or(types::Value::Null)
                        })?,
                    },
                },
            }
        };
        values.push(value);
    }
    Ok(Row {
        values,
        _marker: std::marker::PhantomData,
    })
}

fn split_sql_batch(sql: &str) -> impl Iterator<Item = &str> {
    sql.split(';').map(str::trim).filter(|s| !s.is_empty())
}
