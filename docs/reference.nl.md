# log-bill referentie

[English](reference.md) · Nederlands

Deze pagina bevat alle details. Voor een overzicht, zie de [README](../README.nl.md).

## Opties

Draai `log-bill --help` of `log-bill scan --help` voor de ingebouwde hulp. Combinaties die niet van toepassing zijn worden geweigerd met exit 3 in plaats van stil genegeerd; zie [Exitcodes en gebruiksfouten](#exitcodes-en-gebruiksfouten).

| Optie | Werkt bij | Wat het doet |
| --- | --- | --- |
| `--format` | scan en diff | Invoerformaat, `text` of `jsonl`. Standaard `text`. |
| `--message-field` | scan en diff, alleen met `--format jsonl` | Neem het patroon uit dit JSON-veld. |
| `--window` | scan; bij diff alleen met `--prices` of `--json` | Tijdvenster van de steekproef, bijvoorbeeld `15m`. Rekent om naar een dag en een maand. |
| `--days-per-month` | scan en diff, alleen met `--window` | Dagen in een maand. Standaard 30. |
| `--prices` | scan en diff, alleen met `--window` | Je eigen prijsbestand (JSON). |
| `--rules` | scan en diff | Simuleer `drop`- en `sample`-regels. |
| `--top` | scan | Toon de N grootste patronen; de rest als één regel `other`. |
| `--explain` | scan, ook met `--json` | Toon per patroon de maskers en de volledige rekensom. |
| `--json` | scan en diff | JSON-uitvoer met `schema_version` 1. |
| `--budget-month` | scan, met `--window` en `--prices` | Exit 1 als de maandelijkse ingest-kosten na de regels boven dit bedrag liggen. De schatting van geïndexeerde events telt niet mee. |
| `--max-new-share` | diff | Exit 1 als nieuwe patronen meer dan PCT procent van de nieuwe bytes zijn. |
| `--retention` | scan, alleen met `--window` | Retentie van geïndexeerde logs in dagen: `3`, `7`, `15` of `30`. Standaard 15 als een index-optie gezet is. |
| `--billing` | scan, alleen met `--window` | Plan voor geïndexeerde logs: `annual`, `monthly` of `ondemand`. Standaard `ondemand` als een index-optie gezet is. |
| `--indexed-price` | scan, alleen met `--window` | USD per 1M geïndexeerde events. Overschrijft de lijstprijs. Zelfde decimale regels als een prijs in het prijsbestand. |
| `--index-rules` | scan, alleen met `--window` | Index-uitsluitingsregels, dezelfde syntax als `--rules`, op het gewicht dat na `--rules` overblijft. |
| `--help` | overal | Toont de hulp en stopt met exit 0. |
| `--version` | hoogste niveau | Toont het versienummer (`log-bill 0.1.2`) en stopt met exit 0. |

## Prijsbestand

Prijzen komen alleen uit een JSON-bestand dat je meegeeft met `--prices` (samen met `--window`). Dit is de synthetische fixture `tests/fixtures/prices.json`:

```json
{
  "currency": "EUR",
  "source": "manual",
  "date": "2026-10-08",
  "ingest_per_gb": 0.10,
  "index_per_gb": 0.05,
  "gb_bytes": 500000000
}
```

`index_per_gb` en `gb_bytes` zijn optioneel. Zonder `gb_bytes` is een GB 1000000000 bytes. `gb_bytes` moet een geheel getal groter dan 0 zijn. Een prijs is dat exacte decimale getal, niet een binaire drijvende-kommawaarde. Een breuk die niet in de accumulator past is geen getal: exit 2, `price file has a price that is not a number`. Een getal buiten het bereik van een binaire float, zoals `1e400`, `-1e400`, `1E+400` of een `1` gevolgd door 309 nullen, is geen ongeldige JSON. Bij een prijs is dat dezelfde melding. Bij `gb_bytes` is het `gb_bytes must be a whole number greater than 0`. Meer dan 128 geneste arrays of objecten is exit 2: `price file is nested too deeply`. Dezelfde sleutel twee keer is exit 2: `price file has a duplicate key`.

## Hoe het rekent

- **Bytes van een regel:** de ruwe bytes van de regel plus 1 voor het regeleinde. Een `\r` vóór de `\n` telt mee als byte. Een laatste regel zonder regeleinde telt ook +1. Lege regels tellen niet.
- **Codering:** invoer is UTF-8. Ongeldige UTF-8-bytes tellen mee en worden getoond als U+FFFD. UTF-16 en UTF-32 worden geweigerd met exit 2, zie [Exitcodes en gebruiksfouten](#exitcodes-en-gebruiksfouten). NUL-bytes worden vóór het maskeren uit het bericht gehaald; als bytes tellen ze wel mee. Eén UTF-8-byte order mark aan het begin van een tekstbestand wordt voor de regeltekst overgeslagen en telt mee in de bytes van de eerste regel, zodat dezelfde regel met en zonder dat teken één patroon is. Stuurtekens in een logregel en in currency, source en date van een prijsbestand worden niet ruw geschreven. Tekstuitvoer toont ze als `\u00XX` (ESC is `\u001b`). Dezelfde tekstuitvoer escaped ook bidi-stuurtekens, zodat ze de regel niet kunnen omdraaien: U+202A–U+202E, U+2066–U+2069, U+200E, U+200F en U+061C. U+202E is `\u202e`. JSON houdt het teken (een stuurteken wordt door JSON ge-escaped; een bidi-teken blijft UTF-8). Het patroon-id is de hash van de gemaskeerde tekst, die tekens inbegrepen.
- **JSON lines:** de bytes zijn die van de hele JSON-regel. Eén UTF-8-byte order mark aan het begin van het bestand wordt overgeslagen bij het parsen en telt mee in de bytes van de eerste regel; in een prijsbestand wordt dezelfde BOM overgeslagen. Het patroon komt uit `--message-field`, of standaard uit het eerste veld dat bestaat van `message`, `msg` of `log`. Zonder berichtveld is het patroon `<NO-MESSAGE>`. Een onbekend veld wordt genegeerd, ook als het getal niet in een binaire float past (zoals `1e400`) of de waarde 200 lagen diep is; die regel telt mee. Staat `message`, `msg`, `log` of het veld van `--message-field` twee keer in de regel, dan geldt de latere waarde.
- **Patroon:** het gemaskeerde bericht (zie [Maskering](#maskering)). Het patroon-id is `P` plus de 16 hextekens van de 64-bit FNV-1a-hash van de patroontekst, dus stabiel tussen runs.
- **Aandeel:** bytes van het patroon gedeeld door de totale bytes, als procent met 1 decimaal, half van nul af afgerond.
- **Venster:** `--window` neemt een getal en `s`, `m`, `h` of `d`. De factor per dag is 86400 gedeeld door het venster in seconden. Een maand is `--days-per-month` dagen, standaard 30.
- **Kosten:** per dag = bytes in de steekproef × factor / bytes per GB × prijs per GB, apart voor ingest en (als opgegeven) index. Per maand = per dag × dagen per maand. Er wordt gerekend met gehele getallen en breuken; afronden gebeurt pas bij het tonen: bedragen op 2 decimalen, GB op 3 decimalen, half van nul af.
- **Regels (`--rules BESTAND`):** één per regel, `drop <regex>` of `sample <fractie> <regex>`. De fractie is het deel dat blijft, van 0 tot (niet tot en met) 1, geschreven met een punt (`0.1`, niet `0,1`), en hooguit 80 tekens. De regex wordt op de ruwe regel getoetst en de eerste regel die past wint. Regels waar niets op past blijven heel. Van gesamplede regels houdt het patroon de verwachte waarde: de som van `1 × fractie` voor de regels en de som van `bytes × fractie` voor de bytes, elk één keer en onafhankelijk naar beneden afgerond. Bij één fractie voor het hele patroon is dat `floor(aantal × fractie)` regels en `floor(bytes × fractie)` bytes. Passen twee fracties op hetzelfde patroon, dan worden de verwachte waarden eerst opgeteld en daarna één keer afgerond. Daardoor kan een patroon op 0 regels en toch nog wat bytes uitkomen. De regex en de ruwe regel worden nooit getoond, alleen het regelnummer in het regelsbestand.
- **Geïndexeerde events:** alleen als `--retention`, `--billing`, `--indexed-price` of `--index-rules` gezet is, en alleen samen met `--window`. Events zijn de ingelezen records. Een lege regel telt niet. Eén JSON-regel is één event, ook als het bericht een regeleinde bevat. De maandfactor is dezelfde als voor de bytes. De telling blijft een exacte breuk tot de weergave: miljoenen events op 3 decimalen, bedragen op 2 decimalen, half van nul af. `--index-rules` gebruikt dezelfde syntax als `--rules` en geldt op het gewicht dat na `--rules` overblijft. Een sample blijft een breuk (889 regels × 0.1 = 88.9 events) en wordt niet naar beneden afgerond tot hele regels. Standaard is 15 dagen en `ondemand`, en alleen als deze schatting aan staat. De ingebouwde prijs in USD per 1M events is gelezen op https://www.datadoghq.com/pricing/list/ op 2026-10-14: 3 dagen 1.06/1.27/1.59, 7 dagen 1.27/1.52/1.91, 15 dagen 1.70/2.04/2.55, 30 dagen 2.50/3.00/3.75 (annual/monthly/ondemand). `--indexed-price` vervangt die cel. Het label is `list price, estimate`, of `custom price, estimate` bij een eigen prijs. Tekst, `--explain` en `--json` tonen events en kosten per maand vóór en na de indexregels, plus de besparing. De besparing rondt de kosten van het verschil één keer af. Indexkosten per patroon worden apart afgerond en kunnen daardoor 1 cent van het totaal afwijken. `--budget-month` telt deze schatting niet mee. Flex Logs en contractprijzen zitten er niet in.
- **Budget:** `--budget-month` toetst op de bytes na `--rules`. De regel `total` en de kosten daarvan tonen de bytes vóór de regels; de besparing staat apart. De schatting van geïndexeerde events hoort niet bij die toets.
- **Diff:** per patroon-id de oude en nieuwe bytes en het aandeel, plus nieuwe en verdwenen patronen. Het aandeel van nieuwe patronen is hun bytes gedeeld door de totale nieuwe bytes.

## Maskering

De maskers worden in deze volgorde op het bericht toegepast. Een geheime sleutel wordt in de oorspronkelijke tekst herkend, vóór een later masker hem kan splitsen:

1. Geheim na een sleutel → `<SECRET>`.
2. Tijdstempel → `<TS>` (ISO 8601 / RFC 3339 met een komma-fractie zoals `2026-10-08T13:55:36,123Z`, `10/Oct/2026:13:55:36 +0200`, `2026-10-08 11:07:44`).
3. UUID → `<UUID>`.
4. IPv4 en IPv6 → `<IP>`.
5. E-mailadres → `<EMAIL>`, ook een gewoon adres, een gequote lokaal deel (`"john.doe"@example.com`, en `"a b"@example.com` met een spatie in de aanhalingstekens) en letters buiten ASCII (`jöhn@exämple.de`).
6. Een woord van 8 of meer hextekens → `<HEX>`, maar alleen als er minstens één letter a-f (of A-F) en minstens één cijfer in staat. Een woord van alleen cijfers, zoals `1699482000`, wordt `<NUM>`.
7. Elk ander getal → `<NUM>`, ook cijfers met een `+` of `-` ervoor of een decimale breuk (`-12.5`, `3.14`, `+31612345678`), cijfers buiten ASCII (`١٢٣٤٥`), en een getal met een afsluitend `!`, `?`, `,` of `;` (`7!`). Een afsluitende punt hoort niet bij het getal (`1699482000.` blijft `<NUM>.`). Een tijd als `13:55:36` is geen getal. Elk ander woord met een cijfer erin, en elk woord van 32 of meer tekens → `<VAR>`.

Meerdere spaties worden één spatie.

**IP-adressen.** Een IPv4-adres is vier decimale octetten zonder voorloopnul. IPv6 wordt per woord herkend: gewoon `2001:db8::1`, `::1`, `::` en volledige adressen met acht groepen worden `<IP>`. `std::fs::read` (twee keer `::`) niet, en ook geen gecomprimeerde vorm met tekst aan beide kanten van `::`, hooguit twee groepen, zonder cijfer en zonder IPv4-staart, zoals `a::b`. Een tijd als `13:55:36` is geen IP. Een IPv4-mapped adres (`::ffff:192.0.2.10`) is één `<IP>`, net als een adres met een zone-index (`fe80::1%eth0`). `[adres]:poort` (`[::1]:8080`, `[2001:db8::1]:443`) en `[adres]` zonder poort zijn één `<IP>`, inclusief de haakjes en de poort.

**Geheimen.** Een sleutel is het woord direct vóór `=`, `:`, een spatie, een tab, een regeleinde of een carriage return, ook in JSON-achtige notatie (`"key": "value"`) en in headers. Bevat dat woord `password`, `passwd`, `secret`, `token`, `api_key`, `apikey`, `authorization` of `bearer`, in elke hoofdlettervorm en ook met `_` of `-` ervoor of erna, dan wordt de waarde altijd `<SECRET>`, ongeacht lengte of cijfers. Een `-` telt als `_`, zodat `x-api-key` het woord `api_key` bevat. `monkey=` en `keyboard=` blijven staan, omdat `key` alleen geen sleutelwoord is. `tokenizer=`, `passwordless=` en `secretary=` worden wel gemaskeerd, omdat anders `sessiontoken=` zou lekken. Een `\uXXXX`-escape in de sleutel is dat teken, zodat `pass\u0077ord` het woord `password` bevat. De sleutel wordt gelezen vóór de UUID-, IP- en e-mailmaskers, zodat `password-123e4567-e89b-12d3-a456-426614174000=` en `password-10.0.0.1=` de waarde nog verbergen, ook als de UUID, het IP-adres of het e-mailadres aan de andere kant staat. Een e-mailadres als sleutel (`a@b.co=`) verbergt de waarde ook; een spatie na een adres, zonder `=` of `:`, niet.

Bij een sleutel die `authorization` bevat (`Authorization`, `Proxy-Authorization`, `"authorization":` enzovoort) wordt de hele waarde, inclusief het schemawoord (`Basic`, `Bearer`, `Digest`, `NTLM`, `Negotiate` of een onbekend schema), één `<SECRET>`, en loopt een ongequote waarde tot het einde van het bericht.

Waar een geheime waarde eindigt: een waarde zonder aanhalingstekens eindigt op de eerste spatie. Een gequote waarde (dubbele of enkele aanhalingstekens) eindigt op het sluitende aanhalingsteken, maar alleen als daar een scheiding op volgt: het einde van het bericht, een komma, `}` of `]` (ook na spaties), of spaties gevolgd door een volgend veld zoals `name=` of `name:`. Een aanhalingsteken zonder zo'n scheiding hoort bij de waarde, en een backslash-escape houdt het volgende teken bij de waarde. Volgt er nergens zo'n afsluiting, dan loopt de waarde tot het einde van het bericht: bij twijfel liever maskeren dan lekken.

Een tab beëindigt een ongequote waarde op dezelfde manier, en een tab is een scheiding overal waar een spatie dat is. Een regeleinde of een carriage return ook: `password`, dan een newline, dan `= rosebud` verbergt de waarde, en een carriage return vóór `:` ook. Een backslash direct vóór `"` of `'` is dat aanhalingsteken, zodat geëscapete JSON in een bericht (`{\"password\":\"open sesame\"}`) de waarde maskeert. Binnen een waarde die met een echt aanhalingsteken opende, houdt de backslash het volgende teken bij de waarde. Een waarde die met `[` of `{` begint loopt tot de bijpassende `]` of `}`, zodat de hele array of het hele object één geheim is (`password=["alpha", "beta"]`, `secret={"a": "hunter", "b": "two"}`). Komt die afsluiting niet, dan loopt de waarde tot het einde van het bericht. Een waarde die zelf een sleutelwoord bevat blijft dat ene geheim (`api_key=tokenHunterWordHere next=ok`, `password=secretPass1`, `password=TokenValueXyzHere`). Het maskeren gaat alleen bij de waarde verder als de hele waarde precies een sleutelwoord is en dat woord zelf een waarde heeft (`token: Bearer abc`). Een e-mailadres slikt geen sleutelwoord aan het eind op dat een geheim inleidt (`id=a@b.co.token=hunterpass`).

Met `--explain` krijgt een telling van `<SECRET>` de tekst `possible secret` ("mogelijk geheim"). In `--json` krijgt zo'n patroon de extra sleutel `note` met die tekst, en de sleutel `secret` in `masks` bevat het aantal.

## Exitcodes en gebruiksfouten

| Code | Betekenis |
| --- | --- |
| 0 | In orde |
| 1 | Budget (`--budget-month`) of drempel (`--max-new-share`) overschreden |
| 2 | Invoerfout: logbestand, JSON, prijsbestand of regelsbestand |
| 3 | Gebruiksfout, of de uitvoer kon niet geschreven worden |

Als de uitvoer niet geschreven kan worden, bijvoorbeeld omdat de schijf vol is, is de exitcode 3 en is stderr `could not write output`. `--help` en `--version` gebruiken diezelfde code; ze eindigen niet met 0 als hun tekst niet weg kan.

Een logbestand, prijsbestand of regelsbestand moet een gewoon bestand zijn. Een FIFO, een map of iets anders wordt geweigerd met exit 2, zonder te wachten tot een schrijver opent. Een `gb_bytes` die een breuk is zoals `0.5`, 0 of negatief, of een getal buiten het bereik van een binaire float, is exit 2; een prijs die tekst is in plaats van een getal, een breuk die niet past, of een prijs buiten dat bereik, geeft `price file has a price that is not a number`. Meer dan 128 geneste arrays of objecten geeft `price file is nested too deeply`. Dezelfde sleutel twee keer geeft `price file has a duplicate key`. `sample 0,1` is geen geldige fractie: exit 2 met `invalid rule on line 1 of the rules file`, zonder de regel te tonen. De meldingen over gewone bestanden noemen het pad niet, en geen foutmelding bevat een ruwe logregel.

Elke rij hieronder is op de binary gedraaid, en een test houdt dat zo. Een rij met exit 3 heeft een lege stdout. De melding is de hele stderr; de binary zet er een regeleinde achter. `\n` in de laatste kolom is een regeleinde in die melding. `--json` verandert een melding niet. Bij `diff` wordt `--explain` eerder getoetst dan `--top`, `--window`, `--days-per-month` en een geldige index-optie, en die melding wint dan. Een ongeldig bedrag, een ongeldig percentage, een ongeldige retentie, billing of indexed-prijs, en `--prices` zonder venster worden eerder getoetst dan `--explain`. Bij `scan` wordt een geldige index-optie zonder `--window` geweigerd voordat het index-regelsbestand wordt geopend.

| Optie | Werkt bij | Geval | Verkeerd | Exit | Melding |
| --- | --- | --- | --- | --- | --- |
| `--format` | scan en diff | Ongeldige waarde. | `scan --format nope file.log` | 3 | `error: invalid value 'nope' for '--format <FORMAT>'\n  [possible values: text, jsonl]\n\nFor more information, try '--help'.\n` |
| `--message-field` | scan | Zonder jsonl. | `scan --message-field message file.log` | 3 | `--message-field only works with --format jsonl` |
| `--message-field` | diff | Zonder jsonl. | `diff --message-field message OLD NEW` | 3 | `--message-field only works with --format jsonl` |
| `--message-field` | scan | `--json` is geen invoerformaat jsonl. | `scan --json --message-field message file.log` | 3 | `--message-field only works with --format jsonl` |
| `--window` | diff | Zonder `--prices` en zonder `--json`. | `diff --window 1m OLD NEW` | 3 | `--window with diff only works together with --prices or --json` |
| `--window` | diff | `--explain` wordt eerder getoetst. | `diff --explain --window 1m OLD NEW` | 3 | `--explain only works with scan` |
| `--window` | scan | 0m is ongeldig. | `scan --window 0m file.log` | 3 | `invalid window` |
| `--window` | scan | 5x is ongeldig. | `scan --window 5x file.log` | 3 | `invalid window` |
| `--window` | scan | De eenheid is niet `s`, `m`, `h` of `d`. | `scan --window 1é file.log` | 3 | `invalid window` |
| `--window` | diff | 0m is ongeldig, vóór de diff-toets. | `diff --window 0m OLD NEW` | 3 | `invalid window` |
| `--days-per-month` | scan | Zonder venster. | `scan --days-per-month 31 file.log` | 3 | `--days-per-month only works together with --window` |
| `--days-per-month` | diff | Zonder venster. | `diff --days-per-month 31 OLD NEW` | 3 | `--days-per-month only works together with --window` |
| `--days-per-month` | diff | `--json` verandert de melding niet. | `diff --json --days-per-month 31 OLD NEW` | 3 | `--days-per-month only works together with --window` |
| `--days-per-month` | diff | `--explain` wint van deze latere toets. | `diff --explain --days-per-month 31 OLD NEW` | 3 | `--explain only works with scan` |
| `--prices` | scan | Zonder venster. | `scan --prices no-prices.json file.log` | 3 | `--prices only works together with --window` |
| `--prices` | diff | Zonder venster. | `diff --prices no-prices.json OLD NEW` | 3 | `--prices only works together with --window` |
| `--prices` | scan | `--json` en `--explain` veranderen de melding niet. | `scan --json --explain --prices no-prices.json file.log` | 3 | `--prices only works together with --window` |
| `--prices` | diff | `--explain` wint hier niet: prijzen worden eerder getoetst. | `diff --explain --prices no-prices.json OLD NEW` | 3 | `--prices only works together with --window` |
| `--top` | diff | Hoort niet bij diff. | `diff --top 1 OLD NEW` | 3 | `--top only works with scan` |
| `--top` | diff | `--json` verandert de melding niet. | `diff --json --top 1 OLD NEW` | 3 | `--top only works with scan` |
| `--explain` | diff | Hoort niet bij diff. | `diff --explain OLD NEW` | 3 | `--explain only works with scan` |
| `--explain` | diff | `--json` verandert de melding niet. | `diff --json --explain OLD NEW` | 3 | `--explain only works with scan` |
| `--budget-month` | diff | Hoort niet bij diff. | `diff --budget-month 1 OLD NEW` | 3 | `the monthly budget only applies to scan` |
| `--budget-month` | diff | `--json` en `--explain` veranderen de melding niet. | `diff --json --explain --budget-month 1 OLD NEW` | 3 | `the monthly budget only applies to scan` |
| `--budget-month` | scan | Zonder venster. | `scan --budget-month 1 file.log` | 3 | `budget requires a window` |
| `--budget-month` | scan | Venster wel, prijzen niet. | `scan --budget-month 1 --window 1m file.log` | 3 | `budget requires a price file` |
| `--budget-month` | scan | Prijzen wel, venster niet. Het venster gaat vóór `--prices`. | `scan --budget-month 1 --prices no-prices.json file.log` | 3 | `budget requires a window` |
| `--budget-month` | scan | 1,5 is geen bedrag. | `scan --budget-month 1,5 file.log` | 3 | `invalid budget` |
| `--budget-month` | diff | abc is ongeldig, vóór de commando-toets. | `diff --budget-month abc OLD NEW` | 3 | `invalid budget` |
| `--max-new-share` | scan | Hoort niet bij scan. | `scan --max-new-share 10 file.log` | 3 | `--max-new-share only works with diff` |
| `--max-new-share` | scan | `--json` en `--explain` veranderen de melding niet. | `scan --json --explain --max-new-share 10 file.log` | 3 | `--max-new-share only works with diff` |
| `--max-new-share` | scan | abc bij scan geeft dezelfde melding, niet ongeldig percentage. | `scan --max-new-share abc file.log` | 3 | `--max-new-share only works with diff` |
| `--max-new-share` | diff | abc is geen percentage. | `diff --max-new-share abc OLD NEW` | 3 | `invalid percentage` |
| `--max-new-share` | diff | `--explain` wint hier niet: het percentage wordt eerder getoetst. | `diff --explain --max-new-share abc OLD NEW` | 3 | `invalid percentage` |
| `--max-new-share` | diff | `--json` verandert de melding niet. | `diff --json --max-new-share abc OLD NEW` | 3 | `invalid percentage` |
| `--retention` | scan | 5 is niet 3, 7, 15 of 30. Eerder getoetst dan het venster. | `scan --retention 5 file.log` | 3 | `invalid retention` |
| `--retention` | diff | 5 is ongeldig, vóór de commando-toets. | `diff --retention 5 OLD NEW` | 3 | `invalid retention` |
| `--retention` | scan | Zonder venster. | `scan --retention 7 file.log` | 3 | `--retention only works together with --window` |
| `--retention` | diff | Hoort niet bij diff. | `diff --retention 7 OLD NEW` | 3 | `--retention only works with scan` |
| `--retention` | diff | `--explain` wint van een geldige retentie. | `diff --explain --retention 7 OLD NEW` | 3 | `--explain only works with scan` |
| `--retention` | diff | `--window` zonder prijzen wordt eerder getoetst. | `diff --window 1m --retention 7 OLD NEW` | 3 | `--window with diff only works together with --prices or --json` |
| `--billing` | scan | yearly is geen plan. | `scan --billing yearly file.log` | 3 | `invalid billing` |
| `--billing` | scan | Zonder venster. | `scan --billing annual file.log` | 3 | `--billing only works together with --window` |
| `--billing` | diff | Hoort niet bij diff. | `diff --billing monthly OLD NEW` | 3 | `--billing only works with scan` |
| `--indexed-price` | scan | nope is geen decimaal. | `scan --indexed-price nope file.log` | 3 | `invalid indexed price` |
| `--indexed-price` | scan | Een minteken is een negatieve prijs. | `scan --indexed-price -1 --window 1s file.log` | 3 | `invalid indexed price` |
| `--indexed-price` | scan | Zonder venster. | `scan --indexed-price 1 file.log` | 3 | `--indexed-price only works together with --window` |
| `--indexed-price` | diff | Hoort niet bij diff. | `diff --indexed-price 1 OLD NEW` | 3 | `--indexed-price only works with scan` |
| `--index-rules` | scan | Zonder venster. Het bestand wordt niet geopend. | `scan --index-rules rules.txt file.log` | 3 | `--index-rules only works together with --window` |
| `--index-rules` | diff | Hoort niet bij diff. | `diff --index-rules rules.txt OLD NEW` | 3 | `--index-rules only works with scan` |
| `--index-rules` | diff | `--explain` wint van het index-regelsbestand. | `diff --explain --index-rules rules.txt OLD NEW` | 3 | `--explain only works with scan` |
| `--index-rules` | scan | Het bestand is niet leesbaar. Het pad wordt niet getoond. |  | 2 | `index rules file is not readable` |
| `--index-rules` | scan | Geen gewoon bestand. |  | 2 | `index rules file is not a regular file` |
| `--index-rules` | scan | Een regel is ongeldig. De regel zelf wordt niet getoond. |  | 2 | `invalid rule on line 1 of the index rules file` |
| logbestand | scan en diff | Geen gewoon bestand (FIFO, map). Geweigerd zonder te wachten. |  | 2 | `file is not a regular file` |
| logbestand | scan en diff | UTF-16 of UTF-32, met of zonder byte order mark. De naam is `UTF-16LE`, `UTF-16BE`, `UTF-32LE` of `UTF-32BE`. |  | 2 | `file is UTF-16LE, not UTF-8; convert it first, for example with iconv -f UTF-16LE -t UTF-8, or in PowerShell with Out-File -Encoding utf8` |
| `--prices` | scan en diff | Prijsbestand is geen gewoon bestand. |  | 2 | `price file is not a regular file` |
| `--rules` | scan en diff | Regelsbestand is geen gewoon bestand. |  | 2 | `rules file is not a regular file` |
| `--prices` | scan en diff | `gb_bytes` is geen geheel getal groter dan 0 (een breuk, 0, negatief, of een getal buiten het bereik van een binaire float). |  | 2 | `gb_bytes must be a whole number greater than 0` |
| `--prices` | scan en diff | Dezelfde sleutel komt twee keer voor. |  | 2 | `price file has a duplicate key` |
| `--prices` | scan en diff | Een prijsbreuk past niet, of het getal ligt buiten het bereik van een binaire float. |  | 2 | `price file has a price that is not a number` |
| `--prices` | scan en diff | Meer dan 128 geneste arrays of objecten. |  | 2 | `price file is nested too deeply` |

## Privacy en veiligheid

- Volledig offline: geen netwerk, geen API-key, geen telemetrie. De dependencies zijn beperkt tot clap, regex en serde_json.
- Patronen houden de vaste woorden van een regel. Tijdstempels, UUID's, IP-adressen, e-mailadressen, geheimen, hex, getallen en andere variabele woorden worden vervangen. Een regel waar niets te vervangen is, zoals `disk full`, wordt helemaal getoond. De uitvoer bevat ook patroon-id's, aantallen, bytegroottes en aandelen, en met `--prices` de valuta, de bron en de datum uit het prijsbestand. Een fout over een logbestand, een prijsbestand of een regelsbestand bevat het pad niet, noch de ruwe logregel, de regel uit het regelsbestand of de inhoud van het prijsbestand.
- Tests stoppen een nepgeheim (een canary) in platte tekst, in JSON-velden, achter `password=` en `Authorization: Bearer`, in een kapotte regel en in een regelsbestand, en controleren dat geen enkele uitvoer (tekst, JSON, `--explain`, diff, stderr) het bevat.
- Een privacytest codeert canaries achter `password=`, `token=` en `Authorization: Bearer` als UTF-16LE, UTF-16BE, UTF-32LE en UTF-32BE, elk met en zonder byte order mark, en controleert dat geen uitvoer ze bevat, ook niet na het weghalen van NUL-bytes.
- Het veld `inputs.paths` van `--json` toont een bestandsnaam die geen geldige UTF-8 is met U+FFFD voor de ongeldige bytes.
- Alle fixtures zijn synthetisch; zie [tests/fixtures/README.md](../tests/fixtures/README.md).
