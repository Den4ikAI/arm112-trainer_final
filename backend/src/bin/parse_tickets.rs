//! Разбор билетов: проверка на всём файле.
//!
//! Запуск: `cargo run --bin parse_tickets -- ../extracted/bilety_96_scenarios.md`

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1)
        .unwrap_or_else(|| "../extracted/bilety_96_scenarios.md".into());
    let text = std::fs::read_to_string(&path)?;
    let calls = arm112::ingest::tickets::parse(&text)?;

    let with_name = calls.iter().filter(|c| c.applicant_name.is_some()).count();
    let with_phone = calls.iter().filter(|c| c.applicant_phone.is_some()).count();
    let with_hidden = calls.iter().filter(|c| c.address_hidden.is_some()).count();

    println!("вызовов: {}", calls.len());
    println!("  с ФИО:            {with_name}");
    println!("  с телефоном:      {with_phone}");
    println!("  со скрытым адресом: {with_hidden}");
    println!();

    for c in calls.iter().take(3) {
        println!("билет {} вызов {}", c.ticket, c.call_no);
        println!("  ситуация: {}", c.situation.chars().take(80).collect::<String>());
        println!("  адрес:    {}", c.address);
        if let Some(h) = &c.address_hidden {
            println!("  скрыт:    {h}");
        }
        println!("  заявитель: {:?} {:?}", c.applicant_name, c.applicant_phone);
        println!();
    }

    println!("── без телефона ──");
    for c in calls.iter().filter(|c| c.applicant_phone.is_none()).take(6) {
        println!("  {}.{}: {}", c.ticket, c.call_no,
                 c.situation.chars().take(70).collect::<String>());
    }
    Ok(())
}
