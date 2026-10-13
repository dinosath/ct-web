use sea_orm::{ActiveValue::NotSet, ActiveValue::Set};
use serde::Deserialize;

#[app::dto(entity = "crate::entity::ActiveModel")]
#[derive(Clone, Debug, Deserialize)]
pub struct NewUser {
    pub name: String,
    pub email: String,
}

impl From<NewUser> for crate::entity::ActiveModel {
    fn from(input: NewUser) -> Self {
        Self {
            id: NotSet,
            name: Set(input.name),
            email: Set(input.email),
        }
    }
}
