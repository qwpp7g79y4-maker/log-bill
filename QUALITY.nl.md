# Kwaliteitsrapport

Deze pagina laat zien hoe elke release van log-bill is gecontroleerd, wat we vonden en wat we hebben opgelost. Het beschrijft onze controles. Het is geen garantie dat de software foutloos of veilig is.

## Nog niet uitgebracht

| | |
|---|---|
| Gecontroleerde commit | [`8ea5157`](https://github.com/qwpp7g79y4-maker/log-bill/commit/8ea5157) |
| Datum | 2026-10-17 |
| Tests | 337 automatische tests, gedraaid in debugmodus, releasemodus en op Rust 1.85 (alle drie: 337 geslaagd, 0 mislukt) |

### Wat is toegevoegd

- Een `fuzz/`-crate (cargo-fuzz, geen deel van het gepubliceerde package of de standaardbuild) met één doel per fase die onbetrouwbare invoer verwerkt:

  | Doel | Eigenschap die het controleert |
  |---|---|
  | `fuzz_diff` | Het vergelijken van twee willekeurige samples geeft nooit een panic; de gemelde totalen komen overeen met de invoer, en geen rij heeft meer bytes dan het totaal van zijn sample |
  | `fuzz_mask` | Het maskeren van willekeurige tekst geeft nooit een panic, en een geheim na een herkende sleutel en scheidingsteken verschijnt nooit in de gemaskeerde uitvoer |
  | `fuzz_money` | Het parsen van prijs, venster en budget uit willekeurige tekst geeft nooit een panic, en elk bedrag dat de kostenberekening oplevert is een niet-negatieve decimaal, nooit NaN |
  | `fuzz_read` | Het lezen van een bestand met willekeurige bytes, in elk formaat, geeft nooit een panic en meldt nooit meer regels dan het bestand regeleinden heeft |
  | `fuzz_render` | Het renderen naar tekst en JSON geeft nooit een panic, elke JSON-body die het oplevert kan terug geparseerd worden, en een gemaskeerd geheim duikt in geen van beide weergaven weer op |
  | `fuzz_rules` | Het parsen van een regelsbestand met willekeurige bytes geeft nooit een panic, en het toepassen ervan maakt de regels of bytes van een patroon nooit groter |

- Property tests (proptest), die meedraaien in de normale testsuite: maskeren is idempotent en volgorde-onafhankelijk, een geheim na een herkende sleutel lekt nooit, bij welke variant van witruimte of `\uXXXX` dan ook, de kostenberekening komt overeen met een eenvoudige referentieberekening, en de parsers voor regels, venster, budget, prijsbestand en regelslezen geven nooit een panic op willekeurige invoer.
- Nieuwe regressietests ([`tests/edge_cases.rs`](https://github.com/qwpp7g79y4-maker/log-bill/blob/main/tests/edge_cases.rs)) voor een leeg bestand, een bestand met alleen regeleinden, een losse `\r` zonder `\n`, een byte order mark zonder iets anders, een rauwe NUL-byte in een JSON-string, een JSON Unicode-escape voor NUL die een sleutel splitst, een dubbele sleutel genest in een prijsbestand-object, een negatieve nul als prijs, en een budget voorbij de grens van een 64-bits float.
- Een `Fuzz build`-taak in CI die alle zes doelen bouwt op nightly Rust; hij bouwt ze, maar draait ze niet (zie "Niet gedekt" hieronder).

### Hoe we controleerden

- `cargo test` en `cargo test --release`: 337 geslaagd, 0 mislukt, beide exit 0.
- `cargo +1.85 test` (oudste ondersteunde Rust-versie): 337 geslaagd, 0 mislukt, exit 0.
- `cargo clippy --all-targets -- -D warnings`: exit 0, geen waarschuwingen, in de hoofdcrate en in `fuzz/`.
- `cargo fmt --check`: exit 0, in de hoofdcrate en in `fuzz/`.
- `cargo +nightly fuzz build --target x86_64-unknown-linux-gnu`: exit 0, alle zes doelen bouwen.
- Elk van de zes doelen is 660 seconden gefuzzed, na elkaar, op één vastgezette CPU-kern:

  | Doel | Looptijd | Uitvoeringen | Crashes |
  |---|---|---|---|
  | `fuzz_diff` | 662 s | 2.042.449 | geen |
  | `fuzz_mask` | 661 s | 534.625 | geen |
  | `fuzz_money` | 662 s | 1.399.109 | geen |
  | `fuzz_read` | 662 s | 5.482.286 | geen |
  | `fuzz_render` | 661 s | 662.055 | geen |
  | `fuzz_rules` | 661 s | 650.763 | geen |

- Geautomatiseerde tests in CI voor deze commit: [run 38972694602](https://github.com/qwpp7g79y4-maker/log-bill/actions/runs/38972694602), succesvol, inclusief de nieuwe fuzz-build-taak.

### Echte bugs gevonden

Geen. Bij het schrijven van de fuzz-doelen en property tests vonden en corrigeerden we één fout in de tests zelf: de fuzz- en property tests voor maskeren noemden fullwidth `＝`/`：` eerst als scheidingstekens die `mask::mask` zelf zou herkennen, maar die omzetting gebeurt al eerder, bij het lezen van een regel, niet in de masker. De testlijsten zijn hierop aangepast; dit is een correctie van het testharnas, geen productbug.

### Niet gedekt

- Dit zijn korte lokale fuzzruns (11 minuten per doel, één CPU-kern), geen continue fuzzing.
- CI bouwt de zes fuzz-doelen bij elke wijziging, maar draait ze niet; de runs van 11 minuten per doel hierboven zijn apart, lokaal gedaan.
- Deze commit heeft nog geen onafhankelijke eindreview gehad; die volgt na deze commit, en de badge "PEPAI gecontroleerd" in de README blijft tot dan ongewijzigd.

## v0.1.2

| | |
|---|---|
| Versie | 0.1.2 ([release](https://github.com/qwpp7g79y4-maker/log-bill/releases/tag/v0.1.2), [crates.io](https://crates.io/crates/log-bill/0.1.2)) |
| Gecontroleerde commit | [`e626c10`](https://github.com/qwpp7g79y4-maker/log-bill/commit/e626c10) |
| Datum | 2026-10-14 |
| Tests | 247 automatische tests, gedraaid in debug- en releasemodus |

### Hoe we controleerden

- Dezelfde controles als bij de vorige versie (hieronder), op elk platform.
- Een tweede externe geautomatiseerde codereview. Elke bevinding die we konden reproduceren is in deze versie opgelost, elk met een regressietest met precies die invoer.
- Geautomatiseerde tests in CI: [run 38539431902](https://github.com/qwpp7g79y4-maker/log-bill/actions/runs/38539431902) op Linux, Windows, macOS op Apple Silicon en Intel, en Rust 1.85, plus `cargo audit` (geen bekende kwetsbaarheden).
- Een verse onafhankelijke eindreview van commit [`76180be`](https://github.com/qwpp7g79y4-maker/log-bill/commit/76180be), die later is dan de releasecommit hierboven en herstellingen uit een eerdere eindreview bevat (juiste melding voor prijzen buiten het float-bereik, musl-tests in CI, een controle op de versie in de docs), tegen een vaste edge-case-checklist, elk punt met een concrete kwade invoer. Die eindigde op DONE, zonder blokkerende bevindingen.
- Nog niet uitgebracht: de code voor indexkosten die na deze versie is toegevoegd, kreeg een verse onafhankelijke eindreview van commit [`7e8f630`](https://github.com/qwpp7g79y4-maker/log-bill/commit/7e8f630) tegen dezelfde vaste edge-case-checklist. Die eindigde op DONE, zonder blokkerende bevindingen. De niet-blokkerende bevindingen (kwaliteitsrapport ontbrak in de releasearchieven, een glibc-controle die ruimer was dan de README) zijn daarna met tests opgelost. Een latere eindreview van commit `416696c` vond een lek van een geheim als een sleutelnaam lookalike-tekens gebruikt (bijvoorbeeld een Cyrillische letter in `password`); dat is opgelost in `f9da99c` met regressietests. Een eindreview van commit `5b41dc4` vond nog twee lekken van dezelfde soort (een Cyrillische `н` in `authorization` of `token`, en een ongeldige byte in een sleutel); beide zijn opgelost met regressietests die elke lookalike en elke kapotte byte op elke positie van elk sleutelwoord proberen. Een eindreview van de gefixte code vond dat één enorme regel meerdere keren in het geheugen stond; regels worden nu bij 1 MiB afgekapt, met tests. Een volgende eindreview vond dat een combinerend teken in een sleutel (`ṕassword`) de sleutel verborg; combinerende tekens en letters met accent worden nu gevouwen, met tests over elk sleutelwoord en elke positie. De volgende eindreview vond dat een fullwidth `=`, `:` of aanhalingsteken de waarde verborg; zulke tekens worden nu vóór het maskeren als ASCII gelezen, met tests. De volgende eindreview van commit [`72b3cef`](https://github.com/qwpp7g79y4-maker/log-bill/commit/72b3cef) vond dat een Unicode-spatie rond `=` en lookalike-letters buiten de tabel de waarde verborgen; dat is opgelost in [`49c2a70`](https://github.com/qwpp7g79y4-maker/log-bill/commit/49c2a70) (elke Unicode-spatie scheidt, lookalikes worden via Unicode-confusables gevouwen), met tests. Een verse onafhankelijke eindreview van commit [`49c2a70`](https://github.com/qwpp7g79y4-maker/log-bill/commit/49c2a70), tegen dezelfde vaste edge-case-checklist met elk punt geprobeerd met een concrete kwade invoer, eindigde op DONE, zonder blokkerende bevindingen.

### Wat we vonden en oplosten

| Probleem | Oplossing |
|---|---|
| Geheimen konden in sommige vormen zichtbaar blijven (na een tab, in ge-escapete JSON, in array- of objectwaarden, in sleutels met `\uXXXX`, na een regeleinde, of als een ander masker de sleutel raakte) | Al deze vormen worden gemaskeerd; een gegenereerde lektest dekt ze |
| Gequote en internationale e-mailadressen, en getallen met teken, decimalen, niet-ASCII-cijfers of leesteken erachter, werden niet gemaskeerd | Ze worden `<EMAIL>` en `<NUM>` |
| Prijzen gingen via binaire drijvende komma, zodat een budgetcontrole kon slagen op een afgeronde prijs; dubbele prijssleutels werden geaccepteerd | Prijzen zijn exacte decimalen; dubbele sleutels, overlopende breuken en zeer diepe nesting geven exit 2 |
| Een volle schijf of een ongeldige `--window` gaf een panic | Nette foutmeldingen met gedocumenteerde exitcodes |
| Stuur- en richtingstekens uit de invoer kwamen rauw in de terminal | Ze worden als zichtbare escapes getoond in tekstuitvoer |
| Een pad dat door een FIFO werd vervangen kon de tool laten wachten; grote bestanden werden in één keer ingelezen | Het geopende bestand wordt gecontroleerd; invoer wordt per regel gelezen |

Alle fixes staan in de [changelog](https://github.com/qwpp7g79y4-maker/log-bill/blob/main/CHANGELOG.md).

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
