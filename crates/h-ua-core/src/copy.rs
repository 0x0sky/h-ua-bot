// © 2026 aiaiaiai · aiaiaiai.org
// SPDX-License-Identifier: MIT

//! Everything the conversation says, one catalog per [`Locale`].
//!
//! A catalog is a struct, so a translation that misses a line does not compile. A line with
//! something to fill in names it in braces, `{kinds}`, and [`fill`] puts the value there.

use crate::category::Category;
use crate::locale::Locale;

/// The conversation's copy in one language.
#[derive(Debug)]
pub struct Copy {
    /// What the bot does, before the two notices.
    pub welcome: &'static str,
    /// What this is and is not. Every welcome carries it.
    pub not_official: &'static str,
    /// What is kept about a person. Every welcome carries it.
    pub privacy: &'static str,
    /// The buttons under the input field, after the two notices.
    pub welcome_buttons: &'static str,
    /// Asks for a position with the share button.
    pub location_prompt: &'static str,
    /// After `⬅️ Меню`.
    pub menu: &'static str,
    /// After `/stop`.
    pub stopped: &'static str,
    /// A command the bot does not have.
    pub unknown_command: &'static str,
    /// Free text and anything else the bot does not understand.
    pub other: &'static str,
    /// A position that cannot be read.
    pub bad_location: &'static str,
    /// A position was kept: `{near}`, `{kinds}`.
    pub saved: &'static str,
    /// A choice was made before any position.
    pub position_first: &'static str,
    /// `/status` before any position.
    pub nothing_known: &'static str,
    /// `/status`: `{near}`, `{kinds}`, `{nearby}`.
    pub status: &'static str,
    /// `/kinds` with a word that is not a kind: `{word}`.
    pub unknown_kind: &'static str,
    /// `/kinds` with words: `{kinds}`.
    pub following: &'static str,
    /// `/nearby так|ні`: `{state}`.
    pub nearby_set: &'static str,
    /// The settings panel's text.
    pub settings: &'static str,
    /// The settings button for threats nearby.
    pub nearby_choice: &'static str,
    /// The last kind cannot be switched off.
    pub keep_one: &'static str,
    /// The closest known place: `{place}`.
    pub near_place: &'static str,
    /// No known place is close.
    pub no_place: &'static str,
    /// Following no kind at all.
    pub nothing: &'static str,
    /// Yes, as `/status` answers it.
    pub yes: &'static str,
    /// No, as `/status` answers it.
    pub no: &'static str,
    /// On, as `/nearby` answers it.
    pub on: &'static str,
    /// Off, as `/nearby` answers it.
    pub off: &'static str,
    /// Drones, in the plural as the word reads mid-sentence.
    pub drone: &'static str,
    /// Guided bombs, likewise.
    pub bomb: &'static str,
    /// Missiles, likewise.
    pub missile: &'static str,
}

impl Copy {
    /// The catalog for a locale.
    pub fn of(locale: Locale) -> &'static Copy {
        match locale {
            // Russian and English follow; until then everyone reads Ukrainian.
            Locale::Uk | Locale::Ru | Locale::En => &UK,
        }
    }

    /// A kind's name, lowercase as it reads mid-sentence.
    pub fn category(&self, category: Category) -> &'static str {
        match category {
            Category::Drone => self.drone,
            Category::Bomb => self.bomb,
            Category::Missile => self.missile,
        }
    }
}

/// Puts each value where its `{name}` is in a line.
pub fn fill(line: &str, values: &[(&str, &str)]) -> String {
    values.iter().fold(line.to_owned(), |line, (name, value)| {
        line.replace(&format!("{{{name}}}"), value)
    })
}

/// Ukrainian.
pub static UK: Copy = Copy {
    welcome: "Я повідомляю про повітряні загрози біля вас: дрони, КАБи, ракети. Лише про ті, що стосуються вашого місця.\n\n\
              Як це працює: ви ділитесь позицією, система обробляє повідомлення про загрози, а я надсилаю лише те, що стосується вашої околиці.",
    not_official: "⚠️ Це неофіційний сервіс. Повідомлення бота не замінюють офіційного оповіщення. Відсутність повідомлення не означає безпеку. Орієнтуйтесь на офіційні сповіщення про повітряну тривогу.",
    privacy: "🔒 Позицію зберігаю лише як грубу клітинку близько 3 км, а не точні координати. Разом із нею зберігається технічний ідентифікатор вашого чату. /stop видаляє підписку й позицію.",
    welcome_buttons: "Кнопки під полем вводу:\n\
                      📍 Оновити позицію\n\
                      🛡 Стан — що я про вас знаю\n\
                      ⚙️ За чим стежити? — дрони, КАБи, ракети, загрози поруч\n\
                      /stop — видалити все",
    location_prompt: "Натисніть кнопку нижче, щоб поділитися позицією. Якщо поділитеся «живою» позицією, я оновлюватиму її, поки ви рухаєтесь.",
    menu: "Головне меню",
    stopped: "Готово: підписку й позицію видалено, повідомлень більше не буде. Технічний ідентифікатор чату лишається в системі, але без підписки нічого не надсилається. Щоб повернутись, надішліть /start і поділіться позицією.",
    unknown_command: "Не знаю такої команди. Надішліть /help, щоб побачити, що вмію.",
    other: "Я розумію команди та позицію. Надішліть /help, щоб побачити, що вмію.",
    bad_location: "Не можу прочитати цю позицію. Спробуйте ще раз.",
    saved: "Позицію збережено. {near}\nТочних координат я не зберігаю, лише клітинку близько 3 км.\n\nЗараз стежу за: {kinds}. Змінити: ⚙️ За чим стежити?",
    position_first: "Спершу поділіться позицією: без неї я не знаю, що вас стосується.",
    nothing_known: "Я ще нічого про вас не знаю. Поділіться позицією, і я почну стежити.",
    status: "Позиція: {near}\nВиди: {kinds}\nЗагрози поруч: {nearby}",
    unknown_kind: "Не розумію «{word}». Доступні види: дрони, каби, ракети.",
    following: "Гаразд, стежу за: {kinds}.",
    nearby_set: "Попередження про загрози поруч: {state}.",
    settings: "⚙️ За чим стежити?\n\nНатисніть, щоб увімкнути чи вимкнути.\n\
               «Поруч» — коли щось летить повз ваше місто або біля нього, а не на нього.",
    nearby_choice: "Загрози поруч",
    keep_one: "Хоча б один вид лишається. Щоб не отримувати нічого: /stop",
    near_place: "Найближче відоме місце: {place}.",
    no_place: "Поруч немає місць з мого словника, тож розпізнаю лише згадані міста.",
    nothing: "нічим",
    yes: "так",
    no: "ні",
    on: "увімкнено",
    off: "вимкнено",
    drone: "дрони",
    bomb: "КАБи",
    missile: "ракети",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_disclaimers_say_the_two_things_that_matter() {
        assert!(UK.not_official.contains("неофіційний сервіс"));
        assert!(UK.not_official.contains("не означає безпеку"));
        assert!(UK.privacy.contains("не точні координати"));
    }

    #[test]
    fn values_go_where_they_are_named() {
        assert_eq!(
            fill("{a} і {b}, знову {a}", &[("a", "1"), ("b", "2")]),
            "1 і 2, знову 1"
        );
        assert_eq!(fill("без змін", &[("a", "1")]), "без змін");
    }

    #[test]
    fn category_names_match_the_catalog() {
        assert_eq!(UK.category(Category::Bomb), "КАБи");
        assert_eq!(Category::Bomb.label(), UK.category(Category::Bomb));
    }
}
