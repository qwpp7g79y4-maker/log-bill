# Kwaliteitsrapport

Deze pagina laat zien hoe elke release van log-bill is gecontroleerd, wat we vonden en wat we hebben opgelost. Het beschrijft onze controles. Het is geen garantie dat de software foutloos of veilig is.

## v0.1.1

| | |
|---|---|
| Versie | 0.1.1 ([release](https://github.com/qwpp7g79y4-maker/log-bill/releases/tag/v0.1.1), [crates.io](https://crates.io/crates/log-bill/0.1.1)) |
| Gecontroleerde commit | [`2054eba`](https://github.com/qwpp7g79y4-maker/log-bill/commit/2054eba) |
| Datum | 2026-10-11 |
| Tests | 197 automatische tests |

### Wat we controleerden

**Codereview.** De code is gelezen aan de hand van negen vaste vragen:
1. Geeft het het juiste antwoord?
2. Wat gebeurt er bij foute of ontbrekende invoer?
3. Wat gebeurt er als het halverwege misgaat, bijvoorbeeld als de uitvoer niet geschreven kan worden?
4. Welke aannames doet het, en staan die beschreven?
5. Is het veilig als dingen tegelijk gebeuren?
6. Kan onbetrouwbare invoer in de uitvoer lekken of schade doen, bijvoorbeeld via verborgen tekens?
7. Blijft het snel en zuinig bij grote invoer?
8. Is elke bewering gedekt door een test?
9. Is de code goed te lezen en aan te passen?

**Invoer van elk platform.** UTF-8 met en zonder byte order mark, UTF-16 en UTF-32, Windows-regeleinden (CRLF) en bestandspaden die geen geldige UTF-8 zijn.

**Tests op elk platform.** Elke wijziging draait alle tests op Linux, Windows, macOS op Apple Silicon en macOS op Intel, plus de oudste ondersteunde Rust-versie (1.85).

**Release.** De release wordt pas gebouwd als de tests op de getagde commit opnieuw slagen. Elk archief krijgt een SHA-256-checksum.

**Afhankelijkheden.** `cargo audit` op 2026-10-12 tegen de RustSec-advisorydatabase (1296 meldingen): geen bekende kwetsbaarheden of waarschuwingen in de 33 crate-afhankelijkheden. Dit draait nu bij elke wijziging in CI.

### Hoe we controleerden

- Onafhankelijke codereview, door reviewers die de code niet schreven.
- Een externe automatische codereview. Al zijn bevindingen zijn in deze versie opgelost.
- Automatische tests in CI. Runs: [release v0.1.1](https://github.com/qwpp7g79y4-maker/log-bill/actions/runs/38164492339), [CI op main](https://github.com/qwpp7g79y4-maker/log-bill/actions/workflows/ci.yml?query=branch%3Amain).
- Controle van afhankelijkheden met `cargo audit`.

We noemen de reviewdiensten hier niet. Het gaat om de controles en hun uitkomst, en alles hieronder is na te gaan in de code en de changelog.

### Wat we vonden en oplosten

| Probleem | Oplossing |
|---|---|
| UTF-16- en UTF-32-invoer werd als UTF-8 gelezen, waardoor NUL-bytes een sleutel als `password=` konden splitsen en een geheim ongemaskeerd door kon komen | Zulke invoer wordt geweigerd met exit 2 en een melding hoe je hem omzet |
| NUL-bytes in UTF-8-tekst konden een sleutel verbergen voor het maskeren | Ze worden vóór het maskeren uit het bericht gehaald; het aantal bytes verandert niet |
| UTF-8-tekst met elke 4 bytes een NUL kon voor UTF-16 worden aangezien | De controle op de codering ziet hem niet meer voor UTF-16 aan |
| Een JSON lines- of prijsbestand dat begint met een UTF-8 byte order mark werd geweigerd | De byte order mark wordt geaccepteerd |
| Bestandsnamen die geen geldige UTF-8 zijn werden geweigerd | Ze worden gelezen; `--json` toont U+FFFD voor ongeldige bytes |
| Op Windows noemde de gebruiksregel `log-bill.exe` | Hij noemt altijd `log-bill` |

Alle fixes zitten in v0.1.1, zie de [changelog](https://github.com/qwpp7g79y4-maker/log-bill/blob/main/CHANGELOG.md#011---2026-10-11).

### Niet gedekt

- UTF-16-tekst zonder byte order mark die grotendeels geen ASCII is, wordt mogelijk niet herkend.
- De kostenschatting gebruikt prijzen die je zelf opgeeft. Het is geen officiële rekening.

## Een probleem melden

Open een [issue](https://github.com/qwpp7g79y4-maker/log-bill/issues), of volg bij beveiligingsproblemen [SECURITY.md](SECURITY.md). Contact: pep_ai@icloud.com.
