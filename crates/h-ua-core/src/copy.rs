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
    /// The buttons under the input field, after the two notices: `{location}`, `{status}`,
    /// `{settings}`.
    pub welcome_buttons: &'static str,
    /// The button that shares a position.
    pub button_location: &'static str,
    /// The button for `/status`.
    pub button_status: &'static str,
    /// The button for `/kinds`, the settings.
    pub button_settings: &'static str,
    /// The button for `/help`.
    pub button_help: &'static str,
    /// The button that leaves the settings for the main keyboard.
    pub button_back: &'static str,
    /// The input field's hint under the main keyboard.
    pub hint_menu: &'static str,
    /// The input field's hint when a position is asked for.
    pub hint_location: &'static str,
    /// The input field's hint under the settings.
    pub hint_settings: &'static str,
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
    /// A position was kept: `{near}`, `{kinds}`, `{settings}`.
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
            Locale::Uk => &UK,
            Locale::Ru => &RU,
            Locale::En => &EN,
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
                      {location}\n\
                      {status} — що я про вас знаю\n\
                      {settings} — дрони, КАБи, ракети, загрози поруч\n\
                      /stop — видалити все",
    button_location: "📍 Оновити позицію",
    button_status: "🛡 Стан",
    button_settings: "⚙️ За чим стежити?",
    button_help: "❓ Довідка",
    button_back: "⬅️ Меню",
    hint_menu: "Команда або кнопка нижче",
    hint_location: "Натисніть 📍, щоб поділитися позицією",
    hint_settings: "Оберіть, за чим стежити",
    location_prompt: "Натисніть кнопку нижче, щоб поділитися позицією. Якщо поділитеся «живою» позицією, я оновлюватиму її, поки ви рухаєтесь.",
    menu: "Головне меню",
    stopped: "Готово: підписку й позицію видалено, повідомлень більше не буде. Технічний ідентифікатор чату лишається в системі, але без підписки нічого не надсилається. Щоб повернутись, надішліть /start і поділіться позицією.",
    unknown_command: "Не знаю такої команди. Надішліть /help, щоб побачити, що вмію.",
    other: "Я розумію команди та позицію. Надішліть /help, щоб побачити, що вмію.",
    bad_location: "Не можу прочитати цю позицію. Спробуйте ще раз.",
    saved: "Позицію збережено. {near}\nТочних координат я не зберігаю, лише клітинку близько 3 км.\n\nЗараз стежу за: {kinds}. Змінити: {settings}",
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

/// Russian.
pub static RU: Copy = Copy {
    welcome: "Я сообщаю о воздушных угрозах рядом с вами: дроны, КАБы, ракеты. Только о тех, что касаются вашего места.\n\n\
              Как это работает: вы делитесь позицией, система обрабатывает сообщения об угрозах, а я присылаю только то, что касается вашей округи.",
    not_official: "⚠️ Это неофициальный сервис. Сообщения бота не заменяют официального оповещения. Отсутствие сообщения не означает безопасность. Ориентируйтесь на официальные оповещения о воздушной тревоге.",
    privacy: "🔒 Позицию храню только как грубую клетку около 3 км, а не точные координаты. Вместе с ней хранится технический идентификатор вашего чата. /stop удаляет подписку и позицию.",
    welcome_buttons: "Кнопки под полем ввода:\n\
                      {location}\n\
                      {status} — что я о вас знаю\n\
                      {settings} — дроны, КАБы, ракеты, угрозы рядом\n\
                      /stop — удалить всё",
    button_location: "📍 Обновить позицию",
    button_status: "🛡 Статус",
    button_settings: "⚙️ За чем следить?",
    button_help: "❓ Справка",
    button_back: "⬅️ Меню",
    hint_menu: "Команда или кнопка ниже",
    hint_location: "Нажмите 📍, чтобы поделиться позицией",
    hint_settings: "Выберите, за чем следить",
    location_prompt: "Нажмите кнопку ниже, чтобы поделиться позицией. Если поделитесь «живой» позицией, я буду обновлять её, пока вы двигаетесь.",
    menu: "Главное меню",
    stopped: "Готово: подписка и позиция удалены, сообщений больше не будет. Технический идентификатор чата остаётся в системе, но без подписки ничего не отправляется. Чтобы вернуться, отправьте /start и поделитесь позицией.",
    unknown_command: "Не знаю такой команды. Отправьте /help, чтобы увидеть, что я умею.",
    other: "Я понимаю команды и позицию. Отправьте /help, чтобы увидеть, что я умею.",
    bad_location: "Не могу прочитать эту позицию. Попробуйте ещё раз.",
    saved: "Позиция сохранена. {near}\nТочных координат я не храню, только клетку около 3 км.\n\nСейчас слежу за: {kinds}. Изменить: {settings}",
    position_first: "Сначала поделитесь позицией: без неё я не знаю, что вас касается.",
    nothing_known: "Я пока ничего о вас не знаю. Поделитесь позицией, и я начну следить.",
    status: "Позиция: {near}\nВиды: {kinds}\nУгрозы рядом: {nearby}",
    unknown_kind: "Не понимаю «{word}». Доступные виды: дроны, КАБы, ракеты.",
    following: "Хорошо, слежу за: {kinds}.",
    nearby_set: "Предупреждения об угрозах рядом: {state}.",
    settings: "⚙️ За чем следить?\n\nНажмите, чтобы включить или выключить.\n\
               «Рядом» — когда что-то летит мимо вашего города или рядом с ним, а не на него.",
    nearby_choice: "Угрозы рядом",
    keep_one: "Хотя бы один вид остаётся. Чтобы не получать ничего: /stop",
    near_place: "Ближайшее известное место: {place}.",
    no_place: "Рядом нет мест из моего словаря, поэтому распознаю только упомянутые города.",
    nothing: "ничем",
    yes: "да",
    no: "нет",
    on: "включено",
    off: "выключено",
    drone: "дроны",
    bomb: "КАБы",
    missile: "ракеты",
};

/// English, for everyone else.
pub static EN: Copy = Copy {
    welcome: "I report air threats near you: drones, guided bombs, missiles. Only those that concern where you are.\n\n\
              How it works: you share your position, the system processes threat reports, and I send only what concerns your area.",
    not_official: "⚠️ This is an unofficial service. The bot's messages do not replace official alerts. No message does not mean you are safe. Follow the official air raid alerts.",
    privacy: "🔒 I keep your position only as a coarse cell of about 3 km, not as exact coordinates. A technical identifier of your chat is kept with it. /stop deletes your subscription and position.",
    welcome_buttons: "Buttons under the input field:\n\
                      {location}\n\
                      {status} — what I know about you\n\
                      {settings} — drones, guided bombs, missiles, threats nearby\n\
                      /stop — delete everything",
    button_location: "📍 Update position",
    button_status: "🛡 Status",
    button_settings: "⚙️ What to follow?",
    button_help: "❓ Help",
    button_back: "⬅️ Menu",
    hint_menu: "A command or a button below",
    hint_location: "Tap 📍 to share your position",
    hint_settings: "Choose what to follow",
    location_prompt: "Tap the button below to share your position. If you share a live location, I will keep it updated as you move.",
    menu: "Main menu",
    stopped: "Done: your subscription and position are deleted, and no more messages will come. The chat's technical identifier stays in the system, but nothing is sent without a subscription. To come back, send /start and share your position.",
    unknown_command: "I don't know that command. Send /help to see what I can do.",
    other: "I understand commands and positions. Send /help to see what I can do.",
    bad_location: "I can't read this position. Please try again.",
    saved: "Position saved. {near}\nI don't keep exact coordinates, only a cell of about 3 km.\n\nNow following: {kinds}. To change: {settings}",
    position_first: "Share your position first: without it I don't know what concerns you.",
    nothing_known: "I don't know anything about you yet. Share your position and I'll start following.",
    status: "Position: {near}\nKinds: {kinds}\nThreats nearby: {nearby}",
    unknown_kind: "I don't understand “{word}”. Available kinds: drones, bombs, missiles.",
    following: "OK, following: {kinds}.",
    nearby_set: "Warnings about threats nearby: {state}.",
    settings: "⚙️ What to follow?\n\nTap to switch on or off.\n\
               “Nearby” means something flying past or close to your town, not at it.",
    nearby_choice: "Threats nearby",
    keep_one: "At least one kind stays on. To receive nothing: /stop",
    near_place: "Nearest known place: {place}.",
    no_place: "No places from my dictionary are nearby, so I only recognise towns that are mentioned.",
    nothing: "nothing",
    yes: "yes",
    no: "no",
    on: "on",
    off: "off",
    drone: "drones",
    bomb: "guided bombs",
    missile: "missiles",
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
    fn every_catalog_keeps_the_notices_and_its_placeholders() {
        for copy in [&UK, &RU, &EN] {
            assert!(copy.not_official.starts_with("⚠️"));
            assert!(copy.privacy.starts_with("🔒") && copy.privacy.contains("/stop"));
            for (line, names) in [
                (copy.saved, &["near", "kinds", "settings"][..]),
                (copy.status, &["near", "kinds", "nearby"]),
                (copy.welcome_buttons, &["location", "status", "settings"]),
                (copy.unknown_kind, &["word"]),
                (copy.following, &["kinds"]),
                (copy.nearby_set, &["state"]),
                (copy.near_place, &["place"]),
            ] {
                for name in names {
                    assert!(line.contains(&format!("{{{name}}}")), "{name} in {line}");
                }
            }
        }
    }

    #[test]
    fn button_labels_are_unique_within_a_locale() {
        for copy in [&UK, &RU, &EN] {
            let labels = [
                copy.button_location,
                copy.button_status,
                copy.button_settings,
                copy.button_help,
            ];
            let unique: std::collections::BTreeSet<_> = labels.iter().collect();
            assert_eq!(unique.len(), labels.len());
        }
    }

    #[test]
    fn each_locale_has_its_own_catalog() {
        assert!(Copy::of(Locale::Ru).not_official.contains("неофициальный"));
        assert!(Copy::of(Locale::En).not_official.contains("unofficial"));
        assert!(Copy::of(Locale::Uk).not_official.contains("неофіційний"));
    }

    #[test]
    fn category_names_match_the_catalog() {
        assert_eq!(UK.category(Category::Bomb), "КАБи");
    }
}
