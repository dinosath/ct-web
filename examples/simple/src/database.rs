use sea_orm::{
    ConnectOptions, ConnectionTrait, Database as SeaDatabase, DatabaseConnection, DbErr, Schema,
};

use crate::{config::DatabaseConfig, entity};

#[app::component]
#[derive(Clone)]
pub struct Database {
    #[app::inject]
    config: DatabaseConfig,
    connection: DatabaseConnection,
}

impl Database {
    pub async fn connect(config: DatabaseConfig) -> Result<Self, DbErr> {
        let mut options = ConnectOptions::new(&config.url);
        options.sqlx_logging(false);
        let connection = SeaDatabase::connect(options).await?;
        let table =
            Schema::new(connection.get_database_backend()).create_table_from_entity(entity::Entity);
        connection.execute(&table).await?;
        Ok(Self { config, connection })
    }

    pub fn connection(&self) -> &DatabaseConnection {
        let _ = &self.config;
        &self.connection
    }
}
