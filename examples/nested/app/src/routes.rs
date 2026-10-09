use crate::entity::User;

pub struct UserController;

impl UserController {
    #[app::route(method = "GET", path = "/users")]
    pub fn list() -> &'static [User] {
        &[]
    }

    #[app::route(method = "GET", path = "/users/{id}")]
    pub fn get(id: u64) -> Option<User> {
        (id != 0).then_some(User { id, name: "example" })
    }
}