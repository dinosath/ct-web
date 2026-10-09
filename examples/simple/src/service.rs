use serde::Deserialize;

#[app::dto(entity = "crate::entity::ActiveModel")]
#[derive(Clone, Debug, Deserialize)]
pub struct NewUser {
    pub name: String,
    pub email: String,
}
