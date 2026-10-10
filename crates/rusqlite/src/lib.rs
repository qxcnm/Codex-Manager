use sqlx::pool::PoolConnection;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow};
use sqlx::{Column, Row as SqlxRow, Sqlite, SqliteConnection, SqlitePool, TypeInfo};
use std::cell::Cell;
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::str::FromStr;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock};
use std::thread::{self, ThreadId};
use std::time::Duration;
use tokio::runtime::{Builder, Runtime};
use tokio::sync::Mutex as AsyncMutex;

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
    active_transaction: Arc<StdMutex<Option<ActiveTransaction>>>,
    /// Last rowid observed on this connection's single physical SQLite
    /// handle, fed by statement results and successful lookups. It is only a
    /// fallback for when the on-demand `SELECT last_insert_rowid()` fails;
    /// the authoritative value is always read from the handle itself.
    last_known_rowid: Arc<AtomicI64>,
}

#[derive(Debug, Clone)]
struct ActiveTransaction {
    owner: ThreadId,
    connection: Arc<AsyncMutex<PoolConnection<Sqlite>>>,
}

impl Connection {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let rt = sqlite_runtime()?;
        let options = sqlite_options_for_path(&path)?;
        let pool = block_on_runtime(&rt, async {
            single_handle_pool_options().connect_with(options).await
        })?;
        Ok(Self {
            rt,
            pool,
            path: Some(path),
            active_transaction: Arc::new(StdMutex::new(None)),
            last_known_rowid: Arc::new(AtomicI64::new(0)),
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        let rt = sqlite_runtime()?;
        let pool = block_on_runtime(&rt, async {
            single_handle_pool_options()
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
            active_transaction: Arc::new(StdMutex::new(None)),
            last_known_rowid: Arc::new(AtomicI64::new(0)),
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
        self.execute_params(sql, Params::into_params(params))
    }

    pub fn execute_batch(&self, sql: &str) -> Result<()> {
        if let Some(connection) = self.active_connection_for_current_thread() {
            self.block_on(async {
                let mut connection = connection.lock().await;
                execute_batch_on_connection(&mut connection, sql).await
            })
        } else {
            self.block_on(async {
                for statement in split_sql_batch(sql) {
                    if !statement.trim().is_empty() {
                        sqlx::query(statement).execute(&self.pool).await?;
                    }
                }
                Ok(())
            })
        }
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
        if self.active_connection_for_current_thread().is_some() {
            return Err(Error::SqliteFailure(
                (),
                Some("transaction already active on this thread".into()),
            ));
        }
        let mut connection = self.block_on(self.pool.acquire())?;
        let begin = self.block_on(async {
            sqlx::query("BEGIN IMMEDIATE")
                .execute(connection.as_mut())
                .await
        });
        if let Err(error) = begin {
            let _ = self.block_on(async move {
                drop(connection);
            });
            return Err(error.into());
        }
        let connection = Arc::new(AsyncMutex::new(connection));
        let owner = thread::current().id();
        {
            let mut active = self
                .active_transaction
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if active.is_some() {
                let _ = self.block_on(async move {
                    let connection = connection.lock().await;
                    drop(connection);
                });
                return Err(Error::SqliteFailure(
                    (),
                    Some("transaction already active".into()),
                ));
            }
            *active = Some(ActiveTransaction {
                owner,
                connection: connection.clone(),
            });
        }
        Ok(Transaction {
            conn: self,
            connection: Some(connection),
            owner,
            active_transaction: self.active_transaction.clone(),
            active: Cell::new(true),
        })
    }

    /// Rowid of the most recent successful INSERT on this connection's
    /// physical SQLite handle, read on demand (`sqlite3_last_insert_rowid`
    /// semantics: UPDATE, DELETE, failed or ignored INSERTs leave it as is).
    ///
    /// On the thread that owns an active transaction the value is read from
    /// the transaction's held handle; otherwise the pool's single handle is
    /// acquired, so callers must not share one `Connection` across threads
    /// between the INSERT and this call. If the lookup itself fails, the last
    /// rowid this connection observed is returned and the error is reported
    /// on stderr instead of panicking.
    pub fn last_insert_rowid(&self) -> i64 {
        let result = if let Some(connection) = self.active_connection_for_current_thread() {
            self.block_on(async {
                let mut connection = connection.lock().await;
                query_last_insert_rowid(connection.as_mut()).await
            })
        } else {
            self.block_on(async {
                let mut connection = self.pool.acquire().await?;
                query_last_insert_rowid(connection.as_mut()).await
            })
        };
        self.resolve_last_insert_rowid(result)
    }

    fn resolve_last_insert_rowid(&self, result: Result<i64>) -> i64 {
        match result {
            Ok(rowid) => {
                self.record_rowid(rowid);
                rowid
            }
            Err(error) => {
                let fallback = self.last_known_rowid.load(Ordering::Relaxed);
                eprintln!(
                    "rusqlite shim: last_insert_rowid lookup failed, using last known rowid {fallback}: {error}"
                );
                fallback
            }
        }
    }

    fn record_rowid(&self, rowid: i64) {
        self.last_known_rowid.store(rowid, Ordering::Relaxed);
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

    fn active_connection_for_current_thread(
        &self,
    ) -> Option<Arc<AsyncMutex<PoolConnection<Sqlite>>>> {
        let active = self
            .active_transaction
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        active.as_ref().and_then(|transaction| {
            (transaction.owner == thread::current().id()).then(|| transaction.connection.clone())
        })
    }

    fn execute_params(&self, sql: &str, params: Vec<types::Value>) -> Result<usize> {
        let outcome = if let Some(connection) = self.active_connection_for_current_thread() {
            self.block_on(async {
                let mut connection = connection.lock().await;
                execute_on_connection(&mut connection, sql, params).await
            })
        } else {
            self.block_on(execute_on_pool(&self.pool, sql, params))
        }?;
        self.record_rowid(outcome.rowid);
        Ok(outcome.affected)
    }

    fn fetch_rows(&self, sql: &str, params: Vec<types::Value>) -> Result<Vec<Row<'static>>> {
        if let Some(connection) = self.active_connection_for_current_thread() {
            self.block_on(async {
                let mut connection = connection.lock().await;
                fetch_rows_on_connection(&mut connection, sql, params).await
            })
        } else {
            self.block_on(fetch_rows_on_pool(&self.pool, sql, params))
        }
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
    if tokio::runtime::Handle::try_current()
        .is_ok_and(|handle| handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread)
    {
        // Release the async executor while the legacy synchronous database
        // facade waits on its shared SQLx runtime. No per-query thread.
        tokio::task::block_in_place(|| rt.block_on(future))
    } else if tokio::runtime::Handle::try_current().is_ok() {
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

/// One `Connection` maps to exactly one physical SQLite handle for its whole
/// life, like a real `sqlite3*`. SQLx's default 30 min max lifetime and
/// 10 min idle timeout would silently swap the handle, dropping
/// connection-scoped state (`last_insert_rowid`, `busy_timeout`,
/// `temp_store`, `journal_size_limit`, ...) and wiping in-memory databases.
fn single_handle_pool_options() -> SqlitePoolOptions {
    SqlitePoolOptions::new()
        .max_connections(1)
        .max_lifetime(None)
        .idle_timeout(None)
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
        let rows = self
            .conn
            .fetch_rows(&self.sql, Params::into_params(params))?;
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
        self.conn
            .execute_params(&self.sql, Params::into_params(params))
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
    connection: Option<Arc<AsyncMutex<PoolConnection<Sqlite>>>>,
    owner: ThreadId,
    active_transaction: Arc<StdMutex<Option<ActiveTransaction>>>,
    active: Cell<bool>,
}

impl<'c> Transaction<'c> {
    pub fn execute<P: Params>(&self, sql: &str, params: P) -> Result<usize> {
        let sql = sql.to_string();
        let params = Params::into_params(params);
        let outcome = self.with_connection(|connection| {
            Box::pin(async move { execute_on_connection(connection, &sql, params).await })
        })?;
        self.conn.record_rowid(outcome.rowid);
        Ok(outcome.affected)
    }

    pub fn execute_batch(&self, sql: &str) -> Result<()> {
        let sql = sql.to_string();
        self.with_connection(|connection| {
            Box::pin(async move { execute_batch_on_connection(connection, &sql).await })
        })
    }

    pub fn query_row<P, F, T>(&self, sql: &str, params: P, f: F) -> Result<T>
    where
        P: Params,
        F: FnOnce(&Row<'_>) -> Result<T>,
    {
        let sql = sql.to_string();
        let params = Params::into_params(params);
        let rows = self.with_connection(|connection| {
            Box::pin(async move { fetch_rows_on_connection(connection, &sql, params).await })
        })?;
        match rows.into_iter().next() {
            Some(row) => f(&row),
            None => Err(Error::QueryReturnedNoRows),
        }
    }

    pub fn prepare<'t>(&'t self, sql: &str) -> Result<TransactionStatement<'t, 'c>> {
        Ok(TransactionStatement {
            tx: self,
            sql: sql.to_string(),
        })
    }

    /// Reads `last_insert_rowid()` on demand from the handle this transaction
    /// holds, never from the pool, so it reflects every INSERT issued through
    /// this transaction, its statements, `execute_batch`, or the owning
    /// `Connection` on the owner thread. Lookup failures fall back to the last
    /// known rowid (see [`Connection::last_insert_rowid`]).
    pub fn last_insert_rowid(&self) -> i64 {
        let result = self.with_connection(|connection| {
            Box::pin(async move { query_last_insert_rowid(connection.as_mut()).await })
        });
        self.conn.resolve_last_insert_rowid(result)
    }

    pub fn commit(mut self) -> Result<()> {
        if self.active.get() {
            let result = self.with_connection(|connection| {
                Box::pin(async move {
                    sqlx::query("COMMIT")
                        .execute(&mut **connection)
                        .await
                        .map(|_| ())
                        .map_err(Error::from)
                })
            });
            if result.is_err() {
                let _ = self.with_connection(|connection| {
                    Box::pin(async move {
                        sqlx::query("ROLLBACK")
                            .execute(&mut **connection)
                            .await
                            .map(|_| ())
                            .map_err(Error::from)
                    })
                });
            }
            self.active.set(false);
            self.clear_active_transaction();
            self.release_connection();
            result
        } else {
            self.release_connection();
            Ok(())
        }
    }

    fn clear_active_transaction(&self) {
        let mut active = self
            .active_transaction
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if active.as_ref().is_some_and(|transaction| {
            transaction.owner == self.owner
                && self
                    .connection
                    .as_ref()
                    .is_some_and(|connection| Arc::ptr_eq(&transaction.connection, connection))
        }) {
            *active = None;
        }
    }

    fn release_connection(&mut self) {
        let Some(connection) = self.connection.take() else {
            return;
        };
        let _ = self.conn.block_on(async move {
            let connection = connection.lock().await;
            drop(connection);
        });
    }

    fn with_connection<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send,
        F: Send
            + for<'a> FnOnce(
                &'a mut PoolConnection<Sqlite>,
            ) -> Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>,
    {
        let connection = self
            .connection
            .as_ref()
            .ok_or_else(|| {
                Error::SqliteFailure((), Some("transaction connection unavailable".to_string()))
            })?
            .clone();
        self.conn.block_on(async move {
            let mut connection = connection.lock().await;
            operation(&mut connection).await
        })
    }
}

impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if self.active.replace(false) {
            let _ = self.with_connection(|connection| {
                Box::pin(async move {
                    sqlx::query("ROLLBACK")
                        .execute(connection.as_mut())
                        .await
                        .map(|_| ())
                        .map_err(Error::from)
                })
            });
            self.clear_active_transaction();
        }
        self.release_connection();
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
    use libsqlite3_sys::{
        sqlite3_backup_finish, sqlite3_backup_init, sqlite3_backup_remaining, sqlite3_backup_step,
        sqlite3_errmsg, SQLITE_BUSY, SQLITE_DONE, SQLITE_LOCKED, SQLITE_OK,
    };
    use std::ffi::CStr;
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
            pages_per_step: i32,
            sleep: Duration,
            progress: Option<fn(i32)>,
        ) -> Result<()> {
            let pages_per_step = pages_per_step.max(1);
            self.source.block_on(async {
                let mut source_connection = self.source.pool.acquire().await?;
                let mut target_connection = self.target.pool.acquire().await?;
                let mut source_handle = source_connection.lock_handle().await?;
                let mut target_handle = target_connection.lock_handle().await?;
                let source_ptr = source_handle.as_raw_handle().as_ptr();
                let target_ptr = target_handle.as_raw_handle().as_ptr();
                let main = c"main".as_ptr();
                let backup = unsafe { sqlite3_backup_init(target_ptr, main, source_ptr, main) };
                if backup.is_null() {
                    let message = unsafe { CStr::from_ptr(sqlite3_errmsg(target_ptr)) }
                        .to_string_lossy()
                        .into_owned();
                    return Err(super::Error::SqliteFailure((), Some(message)));
                }
                let result = loop {
                    let code = unsafe { sqlite3_backup_step(backup, pages_per_step) };
                    if let Some(progress) = progress {
                        progress(unsafe { sqlite3_backup_remaining(backup) });
                    }
                    match code {
                        SQLITE_DONE => break Ok(()),
                        SQLITE_OK => continue,
                        SQLITE_BUSY | SQLITE_LOCKED => {
                            if !sleep.is_zero() {
                                std::thread::sleep(sleep);
                            }
                        }
                        _ => {
                            let message = unsafe { CStr::from_ptr(sqlite3_errmsg(target_ptr)) }
                                .to_string_lossy()
                                .into_owned();
                            break Err(super::Error::SqliteFailure((), Some(message)));
                        }
                    }
                };
                let finish_code = unsafe { sqlite3_backup_finish(backup) };
                result?;
                if finish_code != SQLITE_OK {
                    let message = unsafe { CStr::from_ptr(sqlite3_errmsg(target_ptr)) }
                        .to_string_lossy()
                        .into_owned();
                    return Err(super::Error::SqliteFailure((), Some(message)));
                }
                Ok(())
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        fn path(label: &str) -> std::path::PathBuf {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            std::env::temp_dir().join(format!(
                "codexmanager-rusqlite-backup-{label}-{}-{nonce}.db",
                std::process::id()
            ))
        }

        #[test]
        fn online_backup_captures_wal_state_and_replaces_target() {
            let source_path = path("source");
            let target_path = path("target");
            let source = Connection::open(&source_path).expect("open source");
            source
                .execute_batch(
                    "PRAGMA journal_mode=WAL;
                     CREATE TABLE sample(id INTEGER PRIMARY KEY,value TEXT NOT NULL);
                     INSERT INTO sample(value) VALUES('from-wal');",
                )
                .expect("write source");
            let mut target = Connection::open(&target_path).expect("open target");
            target
                .execute_batch("CREATE TABLE stale(value TEXT); INSERT INTO stale VALUES('old');")
                .expect("write stale target");
            Backup::new(&source, &mut target)
                .expect("create backup")
                .run_to_completion(1, Duration::from_millis(1), None)
                .expect("run online backup");
            let value: String = target
                .query_row("SELECT value FROM sample", [], |row| row.get(0))
                .expect("read backed up row");
            assert_eq!(value, "from-wal");
            let stale_exists: i64 = target
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='stale'",
                    [],
                    |row| row.get(0),
                )
                .expect("inspect target schema");
            assert_eq!(stale_exists, 0);
            drop(target);
            drop(source);
            for path in [
                source_path.clone(),
                source_path.with_extension("db-wal"),
                source_path.with_extension("db-shm"),
                target_path.clone(),
                target_path.with_extension("db-wal"),
                target_path.with_extension("db-shm"),
            ] {
                let _ = fs::remove_file(path);
            }
        }
    }
}

#[cfg(test)]
mod transaction_tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn transaction_keeps_shared_handle_out_until_commit() {
        let connection = Connection::open_in_memory().expect("open database");
        connection
            .execute_batch("CREATE TABLE entries(value TEXT NOT NULL);")
            .expect("create table");
        let shared = connection.clone();
        let transaction = connection
            .unchecked_transaction()
            .expect("begin transaction");
        transaction
            .execute("INSERT INTO entries(value) VALUES (?)", ["transaction"])
            .expect("write transaction row");

        let (sender, receiver) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            let result = shared.execute("INSERT INTO entries(value) VALUES (?)", ["shared"]);
            sender.send(result).expect("send worker result");
        });

        assert!(receiver.recv_timeout(Duration::from_millis(100)).is_err());
        transaction.commit().expect("commit transaction");
        worker.join().expect("join worker");
        receiver
            .recv()
            .expect("receive worker result")
            .expect("write shared row");

        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .expect("count rows");
        assert_eq!(count, 2);
    }

    #[test]
    fn transaction_rolls_back_and_keeps_last_insert_rowid_on_owned_connection() {
        let connection = Connection::open_in_memory().expect("open database");
        connection
            .execute_batch("CREATE TABLE entries(id INTEGER PRIMARY KEY, value TEXT NOT NULL);")
            .expect("create table");
        {
            let transaction = connection
                .unchecked_transaction()
                .expect("begin transaction");
            transaction
                .execute("INSERT INTO entries(value) VALUES (?)", ["rolled-back"])
                .expect("write transaction row");
            assert_eq!(transaction.last_insert_rowid(), 1);
        }
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .expect("count rows");
        assert_eq!(count, 0);
    }

    #[test]
    fn connection_methods_reuse_the_active_transaction_on_the_owner_thread() {
        let connection = Connection::open_in_memory().expect("open database");
        connection
            .execute_batch("CREATE TABLE entries(value TEXT NOT NULL);")
            .expect("create table");
        let transaction = connection
            .unchecked_transaction()
            .expect("begin transaction");
        connection
            .execute("INSERT INTO entries(value) VALUES (?)", ["routed"])
            .expect("write through connection route");
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .expect("read through connection route");
        assert_eq!(count, 1);
        drop(transaction);
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .expect("count rolled back rows");
        assert_eq!(count, 0);
    }
}

#[cfg(test)]
mod single_handle_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn assert_never_recycled(connection: &Connection) {
        let options = connection.pool.options();
        assert_eq!(options.get_max_connections(), 1);
        assert_eq!(options.get_max_lifetime(), None);
        assert_eq!(options.get_idle_timeout(), None);
    }

    fn pragma(connection: &Connection, name: &str) -> i64 {
        connection
            .query_row(&format!("PRAGMA {name}"), [], |row| row.get(0))
            .expect("read pragma")
    }

    /// Exercise acquire/release of the pooled handle through every route.
    fn churn(connection: &Connection) {
        for round in 0..20 {
            connection
                .execute("INSERT INTO entries(value) VALUES (?1)", [round])
                .expect("pool insert");
            let _: i64 = connection
                .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
                .expect("pool read");
            connection
                .execute_batch("UPDATE entries SET value = value; SELECT 1;")
                .expect("pool batch");
            let tx = connection.unchecked_transaction().expect("begin");
            tx.execute("UPDATE entries SET value = value + 0", [])
                .expect("tx update");
            if round % 2 == 0 {
                tx.commit().expect("commit");
            }
        }
    }

    #[test]
    fn pools_never_recycle_their_single_handle() {
        assert_never_recycled(&Connection::open_in_memory().expect("open memory"));
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "codexmanager-rusqlite-pool-{}-{nonce}.db",
            std::process::id()
        ));
        assert_never_recycled(&Connection::open(&path).expect("open file"));
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[test]
    fn in_memory_data_survives_repeated_acquire_and_release() {
        let connection = Connection::open_in_memory().expect("open memory");
        connection
            .execute_batch("CREATE TABLE entries(id INTEGER PRIMARY KEY, value INTEGER NOT NULL);")
            .expect("create table");
        churn(&connection);
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .expect("count rows");
        assert_eq!(count, 20);
    }

    #[test]
    fn connection_scoped_pragmas_persist_across_calls() {
        let connection = Connection::open_in_memory().expect("open memory");
        // Mirrors codexmanager-core `Storage::configure_connection`.
        connection
            .busy_timeout(Duration::from_millis(3000))
            .expect("busy timeout");
        connection
            .execute_batch(
                "PRAGMA temp_store=MEMORY; PRAGMA foreign_keys=ON; PRAGMA journal_size_limit=67108864;",
            )
            .expect("configure pragmas");
        connection
            .execute_batch("CREATE TABLE entries(id INTEGER PRIMARY KEY, value INTEGER NOT NULL);")
            .expect("create table");
        churn(&connection);
        assert_eq!(pragma(&connection, "busy_timeout"), 3000);
        assert_eq!(pragma(&connection, "temp_store"), 2);
        assert_eq!(pragma(&connection, "foreign_keys"), 1);
        assert_eq!(pragma(&connection, "journal_size_limit"), 67_108_864);
    }
}

#[cfg(test)]
mod last_insert_rowid_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    const SCHEMA: &str =
        "CREATE TABLE entries(id INTEGER PRIMARY KEY, value TEXT NOT NULL UNIQUE);";

    fn memory() -> Connection {
        let connection = Connection::open_in_memory().expect("open database");
        connection.execute_batch(SCHEMA).expect("create table");
        connection
    }

    fn temp_path(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "codexmanager-rusqlite-rowid-{label}-{}-{nonce}.db",
            std::process::id()
        ))
    }

    fn remove_db(path: &Path) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
    }

    #[test]
    fn fresh_connection_reports_zero() {
        let connection = memory();
        assert_eq!(connection.last_insert_rowid(), 0);
    }

    #[test]
    fn only_successful_inserts_move_the_rowid_outside_transactions() {
        let connection = memory();
        connection
            .execute("INSERT INTO entries(value) VALUES (?1)", ["a"])
            .expect("insert a");
        assert_eq!(connection.last_insert_rowid(), 1);
        connection
            .execute("INSERT INTO entries(id, value) VALUES (?1, ?2)", (40, "b"))
            .expect("insert b");
        assert_eq!(connection.last_insert_rowid(), 40);

        connection
            .execute("UPDATE entries SET value = 'a2' WHERE id = 1", [])
            .expect("update");
        assert_eq!(connection.last_insert_rowid(), 40);
        connection
            .execute("DELETE FROM entries WHERE id = 1", [])
            .expect("delete");
        assert_eq!(connection.last_insert_rowid(), 40);
        let ignored = connection
            .execute("INSERT OR IGNORE INTO entries(value) VALUES (?1)", ["b"])
            .expect("ignored insert");
        assert_eq!(ignored, 0);
        assert_eq!(connection.last_insert_rowid(), 40);
        assert!(connection
            .execute("INSERT INTO entries(value) VALUES (?1)", ["b"])
            .is_err());
        assert_eq!(connection.last_insert_rowid(), 40);
        let _: i64 = connection
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .expect("read");
        assert_eq!(connection.last_insert_rowid(), 40);
    }

    #[test]
    fn transaction_reads_rowid_on_demand_from_its_own_handle() {
        let connection = memory();
        connection
            .execute("INSERT INTO entries(id, value) VALUES (7, 'before')", [])
            .expect("insert before transaction");
        let tx = connection.unchecked_transaction().expect("begin");
        // Same handle as the connection, so the pre-transaction value shows.
        assert_eq!(tx.last_insert_rowid(), 7);
        for (expected, value) in [(8, "x"), (9, "y"), (10, "z")] {
            tx.execute("INSERT INTO entries(value) VALUES (?1)", [value])
                .expect("insert in transaction");
            assert_eq!(tx.last_insert_rowid(), expected);
        }
        tx.execute("UPDATE entries SET value = 'x2' WHERE id = 8", [])
            .expect("update");
        tx.execute("DELETE FROM entries WHERE id = 9", [])
            .expect("delete");
        tx.execute("INSERT OR IGNORE INTO entries(value) VALUES ('z')", [])
            .expect("ignored insert");
        assert_eq!(tx.last_insert_rowid(), 10);

        tx.prepare("INSERT INTO entries(value) VALUES (?1)")
            .expect("prepare")
            .execute(["stmt"])
            .expect("statement insert");
        assert_eq!(tx.last_insert_rowid(), 11);

        // Connection methods on the owner thread route through the same
        // handle without acquiring from the pool (which would deadlock).
        connection
            .execute("INSERT INTO entries(value) VALUES ('routed')", [])
            .expect("routed insert");
        assert_eq!(tx.last_insert_rowid(), 12);
        assert_eq!(connection.last_insert_rowid(), 12);
        tx.commit().expect("commit");

        // The handle keeps its value after COMMIT, like sqlite3_last_insert_rowid.
        assert_eq!(connection.last_insert_rowid(), 12);
    }

    #[test]
    fn execute_batch_inserts_are_visible() {
        let connection = memory();
        connection
            .execute_batch(
                "INSERT INTO entries(value) VALUES ('a'); INSERT INTO entries(value) VALUES ('b');",
            )
            .expect("batch outside transaction");
        assert_eq!(connection.last_insert_rowid(), 2);

        let tx = connection.unchecked_transaction().expect("begin");
        tx.execute_batch(
            "INSERT INTO entries(id, value) VALUES (20, 'c'); UPDATE entries SET value = 'c2' WHERE id = 20;",
        )
        .expect("batch in transaction");
        assert_eq!(tx.last_insert_rowid(), 20);
        tx.commit().expect("commit");
        assert_eq!(connection.last_insert_rowid(), 20);
    }

    #[test]
    fn rollback_keeps_the_handle_value() {
        let connection = memory();
        {
            let tx = connection.unchecked_transaction().expect("begin");
            tx.execute("INSERT INTO entries(id, value) VALUES (5, 'gone')", [])
                .expect("insert");
            assert_eq!(tx.last_insert_rowid(), 5);
        }
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM entries", [], |row| row.get(0))
            .expect("count");
        assert_eq!(count, 0);
        // SQLite does not reset last_insert_rowid on ROLLBACK.
        assert_eq!(connection.last_insert_rowid(), 5);
    }

    #[test]
    fn independent_connections_do_not_share_rowids() {
        let first = memory();
        let second = memory();
        first
            .execute("INSERT INTO entries(id, value) VALUES (3, 'first')", [])
            .expect("insert first");
        assert_eq!(second.last_insert_rowid(), 0);
        second
            .execute("INSERT INTO entries(id, value) VALUES (9, 'second')", [])
            .expect("insert second");
        assert_eq!(first.last_insert_rowid(), 3);
        assert_eq!(second.last_insert_rowid(), 9);
    }

    #[test]
    fn file_connections_on_one_database_keep_their_own_rowids() {
        let path = temp_path("shared-file");
        {
            let writer = Connection::open(&path).expect("open writer");
            writer.execute_batch(SCHEMA).expect("create table");
            let other = Connection::open(&path).expect("open other");

            let tx = writer.unchecked_transaction().expect("begin writer");
            tx.execute("INSERT INTO entries(id, value) VALUES (100, 'w')", [])
                .expect("writer insert");
            tx.commit().expect("commit writer");

            other
                .execute("INSERT INTO entries(id, value) VALUES (200, 'o')", [])
                .expect("other insert");
            assert_eq!(writer.last_insert_rowid(), 100);
            assert_eq!(other.last_insert_rowid(), 200);
        }
        remove_db(&path);
    }

    #[test]
    fn statement_execute_updates_the_connection_rowid() {
        let connection = memory();
        let mut statement = connection
            .prepare("INSERT INTO entries(value) VALUES (?1)")
            .expect("prepare");
        statement.execute(["one"]).expect("first");
        statement.execute(["two"]).expect("second");
        assert_eq!(connection.last_insert_rowid(), 2);
    }
}

/// Result of one executed statement. `rowid` is the handle's
/// `sqlite3_last_insert_rowid()` right after the statement, which SQLx
/// captures for free; it only seeds the lookup fallback.
struct ExecuteOutcome {
    affected: usize,
    rowid: i64,
}

impl From<sqlx::sqlite::SqliteQueryResult> for ExecuteOutcome {
    fn from(result: sqlx::sqlite::SqliteQueryResult) -> Self {
        Self {
            affected: result.rows_affected() as usize,
            rowid: result.last_insert_rowid(),
        }
    }
}

async fn execute_on_pool(
    pool: &SqlitePool,
    sql: &str,
    params: Vec<types::Value>,
) -> Result<ExecuteOutcome> {
    let (sql, params) = normalize_sql_and_params(sql, params)?;
    let result = bind_values(sqlx::query(&sql), &params)
        .execute(pool)
        .await?;
    Ok(result.into())
}

async fn execute_on_connection(
    connection: &mut PoolConnection<Sqlite>,
    sql: &str,
    params: Vec<types::Value>,
) -> Result<ExecuteOutcome> {
    let (sql, params) = normalize_sql_and_params(sql, params)?;
    let result = bind_values(sqlx::query(&sql), &params)
        .execute(connection.as_mut())
        .await?;
    Ok(result.into())
}

async fn query_last_insert_rowid(connection: &mut SqliteConnection) -> Result<i64> {
    sqlx::query_scalar::<_, i64>("SELECT last_insert_rowid()")
        .fetch_one(connection)
        .await
        .map_err(Error::from)
}

async fn execute_batch_on_connection(
    connection: &mut PoolConnection<Sqlite>,
    sql: &str,
) -> Result<()> {
    for statement in split_sql_batch(sql) {
        if !statement.trim().is_empty() {
            sqlx::query(statement).execute(connection.as_mut()).await?;
        }
    }
    Ok(())
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

async fn fetch_rows_on_connection(
    connection: &mut PoolConnection<Sqlite>,
    sql: &str,
    params: Vec<types::Value>,
) -> Result<Vec<Row<'static>>> {
    let (sql, params) = normalize_sql_and_params(sql, params)?;
    let rows = bind_values(sqlx::query(&sql), &params)
        .fetch_all(connection.as_mut())
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
