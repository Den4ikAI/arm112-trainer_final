// Миграции вшиваются в бинарник макросом `sqlx::migrate!` при компиляции,
// но cargo не знает, что папка `migrations` — часть исходников. Без этой
// строки новая миграция без правок Rust-кода в сервер не попадает, пока
// пересборку не вызовет что-то другое, — а сервер на уже обновлённой
// базе падает с «migration … was previously applied but is missing».
fn main() {
    println!("cargo:rerun-if-changed=migrations");
}
