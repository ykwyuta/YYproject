//! Backend dispatch and consistent, lossless transfer between SQLite and PostgreSQL.
use super::*;
use postgres::{Client, IsolationLevel};
use std::cell::RefCell;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DatabaseDump {
    pub projects: Vec<Project>,
    pub audit: Vec<AuditRecord>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuditRecord {
    pub id: i64,
    pub project_id: i64,
    pub at: String,
    pub actor: String,
    pub revision: i64,
    pub change: serde_json::Value,
}
impl DatabaseDump {
    fn validate(&self) -> Result<()> {
        let mut projects = BTreeMap::new();
        for p in &self.projects {
            p.validate()?;
            if p.id <= 0 || p.revision <= 0 || projects.insert(p.id, p.revision).is_some() {
                return Err(invalid("Invalid transfer project IDs/revisions"));
            }
        }
        let mut ids = BTreeSet::new();
        let mut revisions = BTreeSet::new();
        let mut latest = BTreeMap::new();
        for a in &self.audit {
            if a.id <= 0
                || !ids.insert(a.id)
                || !revisions.insert((a.project_id, a.revision))
                || a.revision <= 0
                || projects.get(&a.project_id).is_none_or(|r| a.revision > *r)
                || a.actor.trim().is_empty()
                || chrono::DateTime::parse_from_rfc3339(&a.at).is_err()
            {
                return Err(invalid("Invalid transfer audit record"));
            }
            latest
                .entry(a.project_id)
                .and_modify(|r: &mut i64| *r = (*r).max(a.revision))
                .or_insert(a.revision);
        }
        if projects != latest {
            return Err(invalid("Audit revisions do not match projects"));
        }
        Ok(())
    }
}
pub struct Store {
    backend: Backend,
}
enum Backend {
    Sqlite(SqliteStore),
    Postgres(PgStore),
}
impl Store {
    pub fn open(path: &Path, read_only: bool) -> Result<Self> {
        Ok(Self {
            backend: Backend::Sqlite(SqliteStore::open(path, read_only)?),
        })
    }
    pub fn connect_postgres(url: &str, read_only: bool) -> Result<Self> {
        Ok(Self {
            backend: Backend::Postgres(PgStore::connect(url, read_only)?),
        })
    }
    pub fn is_postgres(&self) -> bool {
        matches!(self.backend, Backend::Postgres(_))
    }
    pub fn list(&self) -> Result<Vec<(i64, String, i64)>> {
        match &self.backend {
            Backend::Sqlite(s) => s.list(),
            Backend::Postgres(s) => s.list(),
        }
    }
    pub fn get(&self, id: i64) -> Result<Project> {
        match &self.backend {
            Backend::Sqlite(s) => s.get(id),
            Backend::Postgres(s) => s.get(id),
        }
    }
    pub fn create(
        &mut self,
        name: String,
        start: NaiveDate,
        finish: NaiveDate,
        actor: &str,
    ) -> Result<Project> {
        match &mut self.backend {
            Backend::Sqlite(s) => s.create(name, start, finish, actor),
            Backend::Postgres(s) => s.create(name, start, finish, actor),
        }
    }
    pub fn apply(
        &mut self,
        id: i64,
        revision: i64,
        change: &Change,
        actor: &str,
    ) -> Result<Project> {
        match &mut self.backend {
            Backend::Sqlite(s) => s.apply(id, revision, change, actor),
            Backend::Postgres(s) => s.apply(id, revision, change, actor),
        }
    }
    pub fn audit(&self, id: i64, after: i64) -> Result<Vec<serde_json::Value>> {
        match &self.backend {
            Backend::Sqlite(s) => s.audit(id, after),
            Backend::Postgres(s) => s.audit(id, after),
        }
    }
    pub fn data_version(&self) -> Result<i64> {
        match &self.backend {
            Backend::Sqlite(s) => s.data_version(),
            Backend::Postgres(s) => s.data_version(),
        }
    }
    pub fn snapshot(&mut self) -> Result<DatabaseDump> {
        let dump = match &mut self.backend {
            Backend::Sqlite(s) => s.snapshot()?,
            Backend::Postgres(s) => s.snapshot()?,
        };
        dump.validate()?;
        Ok(dump)
    }
    /// Imports only into an empty application store; never merges or overwrites IDs.
    pub fn import(&mut self, dump: &DatabaseDump) -> Result<()> {
        dump.validate()?;
        match &mut self.backend {
            Backend::Sqlite(s) => s.import(dump),
            Backend::Postgres(s) => s.import(dump),
        }
    }
    pub fn export_sqlite(&mut self, path: &Path) -> Result<()> {
        if path.exists() {
            return Err(invalid("SQLite destination must be a new file"));
        }
        let dump = self.snapshot()?;
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let file = tempfile::NamedTempFile::new_in(parent)?;
        {
            let mut target = SqliteStore::open(file.path(), false)?;
            target.import(&dump)?;
            target
                .connection
                .execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
        }
        file.persist_noclobber(path)
            .map_err(|e| Error::Io(e.error))?;
        Ok(())
    }
}
impl SqliteStore {
    fn snapshot(&mut self) -> Result<DatabaseDump> {
        let tx = self.connection.transaction()?;
        let mut projects = Vec::new();
        {
            let mut q = tx.prepare("SELECT id,name,revision,document FROM projects ORDER BY id")?;
            let mut rows = q.query([])?;
            while let Some(row) = rows.next()? {
                let p: Project = serde_json::from_str(&row.get::<_, String>(3)?)?;
                if p.id != row.get::<_, i64>(0)?
                    || p.name != row.get::<_, String>(1)?
                    || p.revision != row.get::<_, i64>(2)?
                {
                    return Err(invalid("Project metadata is corrupt"));
                }
                projects.push(p);
            }
        }
        let mut audit = Vec::new();
        {
            let mut q =
                tx.prepare("SELECT id,project_id,at,actor,revision,change FROM audit ORDER BY id")?;
            let mut rows = q.query([])?;
            while let Some(r) = rows.next()? {
                audit.push(AuditRecord {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    at: r.get(2)?,
                    actor: r.get(3)?,
                    revision: r.get(4)?,
                    change: serde_json::from_str(&r.get::<_, String>(5)?)?,
                });
            }
        }
        tx.commit()?;
        Ok(DatabaseDump { projects, audit })
    }
    fn import(&mut self, dump: &DatabaseDump) -> Result<()> {
        if self.read_only {
            return Err(invalid("Read-only database"));
        }
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        if tx.query_row(
            "SELECT (SELECT count(*) FROM projects)+(SELECT count(*) FROM audit)",
            [],
            |r| r.get::<_, i64>(0),
        )? != 0
        {
            return Err(invalid("Migration destination must be empty"));
        }
        for p in &dump.projects {
            tx.execute(
                "INSERT INTO projects(id,name,revision,document) VALUES(?,?,?,?)",
                params![p.id, p.name, p.revision, serde_json::to_string(p)?],
            )?;
        }
        for a in &dump.audit {
            tx.execute(
                "INSERT INTO audit(id,project_id,at,actor,revision,change) VALUES(?,?,?,?,?,?)",
                params![
                    a.id,
                    a.project_id,
                    a.at,
                    a.actor,
                    a.revision,
                    serde_json::to_string(&a.change)?
                ],
            )?;
        }
        tx.pragma_update(None, "user_version", 4)?;
        tx.commit()?;
        Ok(())
    }
}
struct PgStore {
    client: RefCell<Client>,
    read_only: bool,
}
impl PgStore {
    fn connect(url: &str, read_only: bool) -> Result<Self> {
        let tls = postgres_native_tls::MakeTlsConnector::new(native_tls::TlsConnector::new()?);
        let mut config: postgres::Config = url.parse()?;
        config
            .connect_timeout(Duration::from_secs(10))
            .application_name("yyproject");
        let mut client = config.connect(tls)?;
        client.batch_execute("SET statement_timeout=15000; SET lock_timeout=5000")?;
        if read_only {
            client.batch_execute("SET default_transaction_read_only=on")?;
        } else {
            let mut tx = client.transaction()?;
            tx.query_one("SELECT pg_advisory_xact_lock(1499025482)", &[])?;
            let exists: bool = tx
                .query_one("SELECT to_regnamespace('yyproject') IS NOT NULL", &[])?
                .get(0);
            if exists {
                let marker: bool = tx
                    .query_one("SELECT to_regclass('yyproject.meta') IS NOT NULL", &[])?
                    .get(0);
                if !marker {
                    return Err(invalid(
                        "Existing yyproject schema has no application marker",
                    ));
                }
            } else {
                tx.batch_execute("CREATE SCHEMA yyproject;
                CREATE TABLE yyproject.meta(singleton BOOLEAN PRIMARY KEY CHECK(singleton), application_id BIGINT NOT NULL,version INTEGER NOT NULL,revision BIGINT NOT NULL);
                INSERT INTO yyproject.meta VALUES(true,1499025482,4,0);
                CREATE TABLE yyproject.projects(id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,name TEXT NOT NULL,revision BIGINT NOT NULL,document TEXT NOT NULL CHECK(jsonb_typeof(document::jsonb)='object'));
                CREATE TABLE yyproject.audit(id BIGINT GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,project_id BIGINT NOT NULL REFERENCES yyproject.projects(id),at TEXT NOT NULL,actor TEXT NOT NULL,revision BIGINT NOT NULL,change TEXT NOT NULL CHECK(jsonb_typeof(change::jsonb)='object'));
                CREATE INDEX audit_project ON yyproject.audit(project_id,id)")?;
            }
            tx.commit()?;
        }
        let row = client.query_one(
            "SELECT application_id,version FROM yyproject.meta WHERE singleton=true",
            &[],
        )?;
        if row.get::<_, i64>(0) != 1499025482 || !matches!(row.get::<_, i32>(1), 3..=4) {
            return Err(invalid("Unsupported PostgreSQL store format"));
        }
        Ok(Self {
            client: RefCell::new(client),
            read_only,
        })
    }
    fn list(&self) -> Result<Vec<(i64, String, i64)>> {
        Ok(self
            .client
            .borrow_mut()
            .query(
                "SELECT id,name,revision FROM yyproject.projects ORDER BY id",
                &[],
            )?
            .iter()
            .map(|r| (r.get(0), r.get(1), r.get(2)))
            .collect())
    }
    fn get(&self, id: i64) -> Result<Project> {
        let r = self
            .client
            .borrow_mut()
            .query_opt(
                "SELECT id,name,revision,document FROM yyproject.projects WHERE id=$1",
                &[&id],
            )?
            .ok_or_else(|| invalid("Project does not exist"))?;
        Self::project(&r)
    }
    fn project(r: &postgres::Row) -> Result<Project> {
        let document: String = r.get(3);
        if document.len() > 16 * 1024 * 1024 {
            return Err(invalid("Project document exceeds 16 MiB"));
        }
        let p: Project = serde_json::from_str(&document)?;
        p.validate()?;
        if p.id != r.get::<_, i64>(0)
            || p.name != r.get::<_, String>(1)
            || p.revision != r.get::<_, i64>(2)
        {
            return Err(invalid("Project metadata is corrupt"));
        }
        Ok(p)
    }
    fn writable(&self, actor: &str) -> Result<()> {
        if self.read_only {
            return Err(invalid("Read-only database"));
        }
        if actor.trim().is_empty() || actor.len() > 256 {
            return Err(invalid("Actor is required (up to 256 bytes)"));
        }
        Ok(())
    }
    fn create(
        &mut self,
        name: String,
        start: NaiveDate,
        finish: NaiveDate,
        actor: &str,
    ) -> Result<Project> {
        self.writable(actor)?;
        let mut p = Project {
            id: 0,
            revision: 1,
            name,
            start,
            finish,
            tasks: vec![],
            masters: vec![],
            dependencies: vec![],
            history: vec![],
            next_task_id: 1,
            next_master_id: 1,
        };
        p.validate()?;
        let mut client = self.client.borrow_mut();
        let mut tx = client.transaction()?;
        p.id = tx
            .query_one(
                "SELECT nextval(pg_get_serial_sequence('yyproject.projects','id'))",
                &[],
            )?
            .get(0);
        p.capture(chrono::Local::now().date_naive());
        tx.execute(
            "INSERT INTO yyproject.projects(id,name,revision,document) VALUES($1,$2,$3,$4)",
            &[&p.id, &p.name, &p.revision, &serde_json::to_string(&p)?],
        )?;
        Self::record(
            &mut tx,
            p.id,
            actor,
            p.revision,
            "{\"operation\":\"create\"}",
        )?;
        tx.commit()?;
        Ok(p)
    }
    fn record(
        tx: &mut postgres::Transaction<'_>,
        id: i64,
        actor: &str,
        revision: i64,
        change: &str,
    ) -> Result<()> {
        tx.execute("INSERT INTO yyproject.audit(project_id,at,actor,revision,change) VALUES($1,$2,$3,$4,$5)",&[&id,&Utc::now().to_rfc3339(),&actor,&revision,&change])?;
        tx.execute(
            "UPDATE yyproject.meta SET revision=revision+1,version=4 WHERE singleton=true",
            &[],
        )?;
        Ok(())
    }
    fn apply(&mut self, id: i64, revision: i64, change: &Change, actor: &str) -> Result<Project> {
        self.writable(actor)?;
        let mut client = self.client.borrow_mut();
        let mut tx = client.transaction()?;
        let r = tx
            .query_opt(
                "SELECT id,name,revision,document FROM yyproject.projects WHERE id=$1 FOR UPDATE",
                &[&id],
            )?
            .ok_or_else(|| invalid("Project does not exist"))?;
        let mut p = Self::project(&r)?;
        if p.revision != revision {
            return Err(Error::Conflict);
        }
        p.change(change)?;
        p.revision = p
            .revision
            .checked_add(1)
            .ok_or_else(|| invalid("Revision exhausted"))?;
        let document = serde_json::to_string(&p)?;
        if document.len() > 16 * 1024 * 1024 {
            return Err(invalid("Project document exceeds 16 MiB"));
        }
        tx.execute(
            "UPDATE yyproject.projects SET name=$1,revision=$2,document=$3 WHERE id=$4",
            &[&p.name, &p.revision, &document, &id],
        )?;
        Self::record(
            &mut tx,
            id,
            actor,
            p.revision,
            &serde_json::to_string(change)?,
        )?;
        tx.commit()?;
        Ok(p)
    }
    fn audit(&self, id: i64, after: i64) -> Result<Vec<serde_json::Value>> {
        self.client.borrow_mut().query("SELECT id,at,actor,revision,change FROM yyproject.audit WHERE project_id=$1 AND id>$2 ORDER BY id LIMIT 100",&[&id,&after])?.iter().map(|r|Ok(serde_json::json!({"id":r.get::<_,i64>(0),"at":r.get::<_,String>(1),"actor":r.get::<_,String>(2),"revision":r.get::<_,i64>(3),"change":serde_json::from_str::<serde_json::Value>(&r.get::<_,String>(4))?}))).collect()
    }
    fn data_version(&self) -> Result<i64> {
        Ok(self
            .client
            .borrow_mut()
            .query_one(
                "SELECT revision FROM yyproject.meta WHERE singleton=true",
                &[],
            )?
            .get(0))
    }
    fn snapshot(&mut self) -> Result<DatabaseDump> {
        let mut client = self.client.borrow_mut();
        let mut tx = client
            .build_transaction()
            .isolation_level(IsolationLevel::RepeatableRead)
            .read_only(true)
            .start()?;
        let projects = tx
            .query(
                "SELECT id,name,revision,document FROM yyproject.projects ORDER BY id",
                &[],
            )?
            .iter()
            .map(Self::project)
            .collect::<Result<Vec<_>>>()?;
        let audit = tx
            .query(
                "SELECT id,project_id,at,actor,revision,change FROM yyproject.audit ORDER BY id",
                &[],
            )?
            .iter()
            .map(|r| {
                Ok(AuditRecord {
                    id: r.get(0),
                    project_id: r.get(1),
                    at: r.get(2),
                    actor: r.get(3),
                    revision: r.get(4),
                    change: serde_json::from_str(&r.get::<_, String>(5))?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        tx.commit()?;
        Ok(DatabaseDump { projects, audit })
    }
    fn import(&mut self, dump: &DatabaseDump) -> Result<()> {
        self.writable("migration")?;
        let mut client = self.client.borrow_mut();
        let mut tx = client.transaction()?;
        tx.batch_execute("LOCK TABLE yyproject.projects, yyproject.audit IN EXCLUSIVE MODE")?;
        let count:i64=tx.query_one("SELECT (SELECT count(*) FROM yyproject.projects)+(SELECT count(*) FROM yyproject.audit)",&[])?.get(0);
        if count != 0 {
            return Err(invalid("Migration destination must be empty"));
        }
        for p in &dump.projects {
            tx.execute(
                "INSERT INTO yyproject.projects(id,name,revision,document) VALUES($1,$2,$3,$4)",
                &[&p.id, &p.name, &p.revision, &serde_json::to_string(p)?],
            )?;
        }
        for a in &dump.audit {
            tx.execute("INSERT INTO yyproject.audit(id,project_id,at,actor,revision,change) VALUES($1,$2,$3,$4,$5,$6)",&[&a.id,&a.project_id,&a.at,&a.actor,&a.revision,&serde_json::to_string(&a.change)?])?;
        }
        tx.batch_execute("SELECT setval(pg_get_serial_sequence('yyproject.projects','id'), COALESCE((SELECT max(id) FROM yyproject.projects),1), EXISTS(SELECT 1 FROM yyproject.projects)); SELECT setval(pg_get_serial_sequence('yyproject.audit','id'), COALESCE((SELECT max(id) FROM yyproject.audit),1), EXISTS(SELECT 1 FROM yyproject.audit)); UPDATE yyproject.meta SET revision=revision+1,version=4 WHERE singleton=true")?;
        tx.commit()?;
        Ok(())
    }
}
