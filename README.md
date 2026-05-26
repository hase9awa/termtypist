# termtypist

Терминальный тренажер печати с управлением с клавиатуры. Вдохновлен Monkeytype, но работает локально, быстро и без выхода из терминала.

## Возможности

- Режимы по времени, словам, цитатам, своему тексту и повтору последнего теста.
- Встроенные английские и русские словари, плюс пользовательские словари.
- Локальная история в SQLite, личные рекорды, графики результатов и тепловая карта клавиатуры.
- Темы, пунктуация, числа, уровни сложности, blind mode и настраиваемые клавиши.
- Поддержка мыши там, где она удобна, но все действия доступны с клавиатуры.

## Установка

Из исходников:

```sh
cargo install --git https://github.com/hase9awa/termtypist --locked
```

Пользователи Arch Linux могут собрать AUR-пакет из `packaging/aur` после замены GitHub-заглушки в `PKGBUILD`.

## Использование

```sh
termtypist
termtypist --time 30
termtypist --words 50 --dictionary english
termtypist quote --length medium
termtypist custom text.txt
cat text.txt | termtypist custom
termtypist stats
termtypist replay last
termtypist theme list
termtypist theme set catppuccin
termtypist config export > config.toml
termtypist config import config.toml
```

Основные клавиши:

| Клавиша | Действие |
| --- | --- |
| `tab` | Рестарт |
| `ctrl+r` | Повторить тот же текст |
| `esc` | Пауза или закрыть окно |
| `f1` | Справка |
| `ctrl+enter`, `f2` | Настройки |
| `alt+t`, `alt+w`, `alt+q` | Режим времени, слов, цитат |
| `alt+d` | Словарь |
| `alt+p`, `alt+n` | Пунктуация, числа |
| `r`, `e` | История, тепловая карта |
| `ctrl+c` | Выход |

## Файлы

termtypist хранит пользовательские данные локально:

- Конфиг, словари, цитаты и темы: `~/.config/termtypist`
- База результатов: каталог данных платформы, файл `termtypist/results.sqlite`

Переменная `TERM_TYPIST_CONFIG_DIR` позволяет задать другой каталог конфига.

Пользовательские словари можно положить в `~/.config/termtypist/languages` в формате `txt`, `json` или `toml`. Пользовательские темы и наборы цитат используют те же форматы в каталогах `themes` и `quotes`.

## Релиз

Перед публикацией проверьте сборку и создайте тег версии:

```sh
cargo test --locked
git tag v0.1.0
git push origin main v0.1.0
```

Для AUR обновите `sha256sums` после появления GitHub-архива:

```sh
cd packaging/aur
updpkgsums
makepkg --printsrcinfo > .SRCINFO
makepkg -si
```

## Лицензия

MIT

---

## English

Keyboard-first typing trainer for the terminal. It is inspired by Monkeytype, but keeps the workflow local, fast, and terminal-native.

### Features

- Time, words, quote, custom text, and replay modes.
- English and Russian dictionaries, with support for custom dictionaries.
- Local SQLite history, personal bests, result charts, and keyboard heatmaps.
- Themes, punctuation and number toggles, difficulty modes, blind mode, and configurable keybindings.
- Mouse support where useful, while every action remains available from the keyboard.

### Install

From source:

```sh
cargo install --git https://github.com/hase9awa/termtypist --locked
```

Arch Linux users can build the AUR package from `packaging/aur` after replacing the GitHub owner placeholder in `PKGBUILD`.

### Usage

```sh
termtypist
termtypist --time 30
termtypist --words 50 --dictionary english
termtypist quote --length medium
termtypist custom text.txt
cat text.txt | termtypist custom
termtypist stats
termtypist replay last
termtypist theme list
termtypist theme set catppuccin
termtypist config export > config.toml
termtypist config import config.toml
```

Default keys:

| Key | Action |
| --- | --- |
| `tab` | Restart |
| `ctrl+r` | Retry the same text |
| `esc` | Pause or close |
| `f1` | Help |
| `ctrl+enter`, `f2` | Settings |
| `alt+t`, `alt+w`, `alt+q` | Time, words, quote mode |
| `alt+d` | Dictionary |
| `alt+p`, `alt+n` | Punctuation, numbers |
| `r`, `e` | History, heatmap |
| `ctrl+c` | Quit |

### Files

termtypist writes user data locally:

- Config, dictionaries, quotes, and themes: `~/.config/termtypist`
- Results database: platform data directory under `termtypist/results.sqlite`

Set `TERM_TYPIST_CONFIG_DIR` to use a different config directory.

Custom dictionaries can be `txt`, `json`, or `toml` files under `~/.config/termtypist/languages`. Custom themes and quote sets use the same formats under `themes` and `quotes`.

### Release

Before publishing a release, verify the build and tag the version:

```sh
cargo test --locked
git tag v0.1.0
git push origin main v0.1.0
```

For AUR, update `sha256sums` after the GitHub archive is available:

```sh
cd packaging/aur
updpkgsums
makepkg --printsrcinfo > .SRCINFO
makepkg -si
```

### License

MIT
