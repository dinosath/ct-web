use sea_orm::DbErr;

use crate::entity::{self, Repository};

#[app::singleton]
pub struct UserService {
    #[app::inject]
    repository: Repository,
}

impl UserService {
    pub async fn list(&self) -> Result<Vec<entity::Model>, DbErr> {
        self.repository.find_all().await
    }

    #[app::transactional]
    pub async fn create(&self, entity: entity::ActiveModel) -> Result<entity::Model, DbErr> {
        self.repository.insert(entity).await
    }

    pub async fn get(&self, id: i32) -> Result<Option<entity::Model>, DbErr> {
        self.repository.find_by_id(id).await
    }

    #[app::transactional]
    pub async fn update(&self, entity: entity::ActiveModel) -> Result<entity::Model, DbErr> {
        self.repository.update(entity).await
    }

    #[app::transactional]
    pub async fn delete(&self, id: i32) -> Result<bool, DbErr> {
        self.repository.delete_by_id(id).await
    }
}
