//! Генерация argon2id-хеша для учебных учётных записей.
//!
//! ```bash
//! cargo run --bin hash_password -- arm112
//! ```

use argon2::Argon2;
use password_hash::PasswordHasher;

fn main() {
    let password = std::env::args().nth(1).unwrap_or_else(|| "arm112".into());
    // Соль генерируется автоматически, своя на каждый вызов
    let hash = Argon2::default()
        .hash_password(password.as_bytes())
        .expect("не удалось захешировать пароль");
    println!("{hash}");
}
