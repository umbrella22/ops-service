//! Authentication and authorization module

pub mod jwt;
pub mod password;
pub mod api_key;
pub mod middleware;

pub use jwt::{Claims, JwtService, TokenPair};
pub use password::PasswordHasher;
pub use api_key::ApiKeyGenerator;
pub use middleware::{AuthContext, extract_token, jwt_auth_middleware, optional_auth_middleware, get_auth_context};
