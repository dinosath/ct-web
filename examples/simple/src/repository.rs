use sea_orm::{ActiveModelTrait, DbErr, EntityTrait};

use crate::{database::Database, entity};

#[app::component]
#[derive(Clone)]
pub struct Repository {
    #[app::inject]
    database: Database,
}

impl Repository {
    pub async fn find_all(&self) -> Result<Vec<entity::Model>, DbErr> {
        entity::Entity::find().all(self.database.connection()).await
    }

    pub async fn find_by_id(&self, id: i32) -> Result<Option<entity::Model>, DbErr> {
        entity::Entity::find_by_id(id)
            .one(self.database.connection())
            .await
    }

    pub async fn insert(&self, model: entity::ActiveModel) -> Result<entity::Model, DbErr> {
        model.insert(self.database.connection()).await
    }

    pub async fn update(&self, model: entity::ActiveModel) -> Result<entity::Model, DbErr> {
        model.update(self.database.connection()).await
    }

    pub async fn delete_by_id(&self, id: i32) -> Result<bool, DbErr> {
        let result = entity::Entity::delete_by_id(id)
            .exec(self.database.connection())
            .await?;
        Ok(result.rows_affected > 0)
    }
}
