use crate::codegen::error::Error as CodegenError;
use crate::container::error::Error as ContainerError;
use crate::pool::error::Error as PoolBuilderError;
use crate::prepare_queries::error::Error as PrepareQueriesError;
use crate::read_queries::error::Error as ReadQueriesError;
use crate::run_migrations::error::Error as MigrationError;

use deadpool_postgres::CreatePoolError;
use thiserror::Error as ThisError;
#[derive(Debug, ThisError)]
#[error("{0}")]
pub(crate) enum Error {
    ReadQueries(#[from] ReadQueriesError),
    Container(#[from] ContainerError),
    Codegen(#[from] CodegenError),
    PrepareQueries(#[from] PrepareQueriesError),
    NewMigration(#[from] std::io::Error),
    Migration(#[from] MigrationError),
    PoolCreation(#[from] CreatePoolError),
    Pool(#[from] deadpool_postgres::PoolError),
    PoolBuilder(#[from] PoolBuilderError),
    Fmt(#[from] FmtError),
}

#[derive(Debug, ThisError)]
#[error("Unable to properly format the generated code.")]
pub(crate) enum FmtError {
    IO(#[from] std::io::Error),
    RustFmt,
}
