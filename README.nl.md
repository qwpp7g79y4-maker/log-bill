# log-bill

[English](README.md) · Nederlands

Schat offline wat je logs per patroon kosten, voordat je ze verstuurt.

[![crates.io](https://img.shields.io/crates/v/log-bill.svg)](https://crates.io/crates/log-bill) [![CI](https://github.com/qwpp7g79y4-maker/log-bill/actions/workflows/ci.yml/badge.svg)](https://github.com/qwpp7g79y4-maker/log-bill/actions/workflows/ci.yml) [![PEPAI gecontroleerd](https://img.shields.io/endpoint?url=https%3A%2F%2Fraw.githubusercontent.com%2Fqwpp7g79y4-maker%2Flog-bill%2Fmain%2F.github%2Fbadge.json)](QUALITY.nl.md)

[Installatie](#installatie) • [Snel aan de slag](#snel-aan-de-slag) • [Hoe het rekent](#hoe-het-rekent) • [Alternatieven](#vergelijking-met-alternatieven) • [Beperkingen](#beperkingen) • [Referentie](docs/reference.nl.md)

log-bill is een opdrachtregelprogramma in Rust. Het leest een steekproef uit een logbestand op je eigen machine (platte tekst of JSON lines), groepeert de regels tot patronen door de variabele delen te maskeren, en laat per patroon zien hoeveel regels en bytes het is en welk aandeel van het volume. Met een tijdvenster en een prijsbestand dat je zelf meegeeft rekent het de steekproef om naar een geschatte prijs per dag en per maand. Het kan `drop`- en `sample`-regels simuleren, twee steekproeven vergelijken, elke rekensom uitleggen, JSON schrijven en een CI-job laten falen als een budget wordt overschreden. Het heeft geen account nodig en maakt geen netwerkverbinding.

De uitvoer van de tool is Engels; de voorbeelden hieronder tonen die precies zoals de binary hem schrijft.

## Waarom

Teams betalen per GB voor logs en zien vaak pas op de factuur welk patroon het volume maakt. Twee opmerkingen op Hacker News, nagelezen op 8 oktober 2026: nthypes schrijft "My bill jumped nearly 1000% before alerts caught it" (https://news.ycombinator.com/item?id=46034896) en mleonhard noemt "the risk of bad logging running up a $70,000/day bill with CloudWatch Logs" (https://news.ycombinator.com/item?id=45630239). log-bill laat vóór het versturen, op een gewoon bestand, zien welke patronen de bytes zijn en wat een drop- of sample-regel zou schelen.

## Installatie

Met Rust 1.85 of nieuwer installeer je vanaf [crates.io](https://crates.io/crates/log-bill):

```sh
cargo install --locked log-bill
log-bill --help
```

Kant-en-klare binaries voor Linux (x86_64; vanaf v0.1.1 ook een statische `musl`-build die geen bepaalde glibc nodig heeft en op Alpine draait), Windows (x86_64) en macOS (Intel en Apple Silicon), elk met een SHA-256-checksum, staan op de [Releases-pagina](https://github.com/qwpp7g79y4-maker/log-bill/releases).

Ondersteunde systemen voor de kant-en-klare binaries:

- **Linux x86_64 (`x86_64-unknown-linux-gnu`):** gebouwd op Ubuntu 22.04, heeft glibc 2.34 of nieuwer nodig (bijvoorbeeld Ubuntu 22.04, Debian 12, RHEL 9 en later).
- **Linux x86_64 (`x86_64-unknown-linux-musl`, vanaf v0.1.1):** statisch gelinkt, geen glibc nodig; voor Alpine en oudere distributies.
- **Windows x86_64 (`x86_64-pc-windows-msvc`).**
- **macOS Intel (`x86_64-apple-darwin`) en Apple Silicon (`aarch64-apple-darwin`).**

De tests draaien in CI op Linux, Windows, macOS op Apple Silicon en macOS op Intel, en op Rust 1.85. Andere systemen kunnen werken met `cargo install`, maar zijn niet getest.

Voor de nieuwste `main`: `cargo install --locked --git https://github.com/qwpp7g79y4-maker/log-bill`.

Vanuit een lokale clone: `cargo install --locked --path .`, of `cargo build --release`; de binary staat dan in `target/release/log-bill`.

## Snel aan de slag

Dit is de echte uitvoer van de binary op de synthetische fixtures in `tests/fixtures`. Een test draait elk voorbeeld en vergelijkt de uitvoer.

Een logbestand groeperen tot patronen:

```console
$ log-bill scan tests/fixtures/scan-text.log
total: 3 lines, 118 bytes
Pe19dc70d8299c276  2 lines  108 bytes  91.5%  user <NUM> failed at <TS> from <IP>
P46ab161881a7d411  1 line  10 bytes  8.5%  disk full
```

Een steekproef van één minuut omrekenen naar een dag en een maand:

```console
$ log-bill scan --window 1m tests/fixtures/scan-text.log
total: 3 lines, 118 bytes
Pe19dc70d8299c276  2 lines  108 bytes  91.5%  user <NUM> failed at <TS> from <IP>
P46ab161881a7d411  1 line  10 bytes  8.5%  disk full
factor: 1440
per day: 169920 bytes, 0.000 GB
per month: 5097600 bytes, 0.005 GB
```

118 bytes in één minuut is × 1440 = 169920 bytes per dag, en × 30 = 5097600 bytes per maand. Met 1000000000 bytes per GB is dat 0.000 GB per dag en 0.005 GB per maand, op drie decimalen.

Geïndexeerde events gebruiken datzelfde venster. Drie records over één seconde, 7 dagen bewaard, op de jaarlijkse lijstprijs:

```console
$ log-bill scan --window 1s --days-per-month 30 --retention 7 --billing annual tests/fixtures/scan-text.log
total: 3 lines, 118 bytes
Pe19dc70d8299c276  2 lines  108 bytes  91.5%  user <NUM> failed at <TS> from <IP>
P46ab161881a7d411  1 line  10 bytes  8.5%  disk full
factor: 86400
per day: 10195200 bytes, 0.010 GB
per month: 305856000 bytes, 0.306 GB
indexed: list price, estimate, retention 7d, billing annual, 1.27 USD per 1M events
indexed events per month: 7.776M before index rules, 7.776M after
indexed cost per month: 9.88 USD before index rules, 9.88 USD after, savings 0.00 USD
indexed Pe19dc70d8299c276: 5.184M -> 5.184M events per month, 6.58 USD -> 6.58 USD, savings 0.00 USD
indexed P46ab161881a7d411: 2.592M -> 2.592M events per month, 3.29 USD -> 3.29 USD, savings 0.00 USD
```

3 records × 30 × 86400 / 1000000 = 7.776 miljoen events per maand. Tegen 1.27 USD per miljoen is dat 9.87552, getoond als 9.88. De synthetische fixture `tests/fixtures/indexed/app.log` (6000 regels) met hetzelfde venster, `--retention 7`, `--billing annual` en een indexregel `drop DEBUG` is 15552.000M events per maand vóór de regel en 6928.416M erna. 2673/6000 × 15552 × 1.27 is 8799.08832, getoond als 8799.09 USD per maand. Met erbij `drop GET /health` en `sample 0.1 INFO order` blijven 982.109M events en 1247.28 USD over. De lijst is gelezen op https://www.datadoghq.com/pricing/list/ op 2026-10-14. Zonder `--retention`, `--billing`, `--indexed-price` of `--index-rules` ontbreekt dit blok.

Een sample-regel simuleren:

```console
$ log-bill scan --rules tests/fixtures/sample-0.1.txt tests/fixtures/sample-zero.log
total: 1 line, 10 bytes
P139ac555f8a88ea7  1 line  10 bytes  100.0%  xxxxxxxxx
after rules: 1 line, 10 bytes -> 0 lines, 1 byte, savings 1 line, 9 bytes
P139ac555f8a88ea7  1 line, 10 bytes -> 0 lines, 1 byte, savings 1 line, 9 bytes, rule 1
```

`sample-zero.log` is negen letters x en een regeleinde: 1 regel, 10 bytes. `sample 0.1` houdt `floor(1 × 0.1) = 0` regels en `floor(10 × 0.1) = 1` byte over.

Twee steekproeven vergelijken:

```console
$ log-bill diff tests/fixtures/diff-old.log tests/fixtures/diff-new.log
total: 3 lines, 17 bytes -> 2 lines, 12 bytes, change -1 line, -5 bytes
new P229176bd1f6ba96a  0 -> 6 bytes, change +6 bytes, share 0.0% -> 50.0%, change +50.0 percentage points  gamma
gone P7627619b954620a7  5 -> 0 bytes, change -5 bytes, share 29.4% -> 0.0%, change -29.4 percentage points  beta
P8ac625bb85ed202b  12 -> 6 bytes, change -6 bytes, share 70.6% -> 50.0%, change -20.6 percentage points  alpha
new patterns: 6 of 12 bytes, 50.0%
```

De oude steekproef is twee keer `alpha` (12 bytes) en één keer `beta` (5 bytes): 17 bytes. De nieuwe is `alpha` (6) en `gamma` (6): 12 bytes. Het nieuwe patroon `gamma` is 6 van 12 bytes, 50.0%.

Prijzen komen alleen uit een JSON-bestand dat je meegeeft met `--prices` (samen met `--window`); zie [het prijsbestand](docs/reference.nl.md#prijsbestand). Elke prijs wordt als exact decimaal getal gelezen. Een breuk die niet past, een getal buiten het bereik van een binaire float, of een bestand dat meer dan 128 arrays of objecten diep is, wordt geweigerd, en een sleutel die twee keer voorkomt ook. Elke tekstuitvoer met bedragen per GB eindigt met `estimate: extrapolation of one sample with your own prices; not an official rate` ("schatting: extrapolatie van één steekproef met je eigen prijzen; geen officieel tarief"). Een schatting van geïndexeerde events heeft het label `list price, estimate`, of `custom price, estimate` als je `--indexed-price` meegeeft. Alle opties, exitcodes en foutmeldingen staan in de [referentie](docs/reference.nl.md) en in `log-bill --help`.

## Hoe het rekent

Elk getal is terug te voeren op deze regels. De volledige maskerregels staan in de [referentie](docs/reference.nl.md#maskering).

- **Bytes:** de ruwe bytes van een regel plus 1 voor het regeleinde. Lege regels tellen niet. Bij JSON lines de bytes van de hele regel.
- **Patroon:** het bericht met de variabele delen gemaskeerd (`<TS>`, `<UUID>`, `<IP>`, `<EMAIL>`, `<SECRET>`, `<HEX>`, `<NUM>`, `<VAR>`). Het id is `P` plus de 16 hextekens van de 64-bit FNV-1a-hash van de patroontekst.
- **Aandeel:** bytes van het patroon gedeeld door de totale bytes, als procent met 1 decimaal, half van nul af afgerond.
- **Venster:** de factor per dag is 86400 gedeeld door het venster in seconden. Een maand is `--days-per-month` dagen, standaard 30.
- **Kosten:** per dag = bytes in de steekproef × factor / bytes per GB × prijs per GB, apart voor ingest en index. Per maand = per dag × dagen per maand. Er wordt gerekend met gehele getallen en breuken; afronden gebeurt pas bij het tonen: bedragen op 2 decimalen, GB op 3 decimalen, half van nul af.
- **Regels:** `drop <regex>` of `sample <fractie> <regex>`, getoetst op de ruwe regel, de eerste die past wint. Een fractie langer dan 80 tekens is ongeldig. Een gesampled patroon houdt de verwachte waarde, één keer naar beneden afgerond voor de regels en één keer voor de bytes, onafhankelijk van elkaar. Daardoor kan een patroon op 0 regels en toch nog wat bytes uitkomen. De regex en de ruwe regel worden nooit getoond.
- **Geïndexeerde events:** optioneel, en alleen met `--window` plus `--retention`, `--billing`, `--indexed-price` of `--index-rules`. Events zijn ingelezen records. Ze gebruiken dezelfde maandfactor als de bytes en blijven een exacte breuk tot de weergave (miljoenen op 3 decimalen, bedragen op 2 decimalen, half van nul af). Indexkosten per patroon worden apart afgerond en kunnen daardoor 1 cent van het totaal afwijken. `--index-rules` heeft dezelfde syntax als `--rules` en geldt na `--rules`; een sample wordt niet naar beneden afgerond tot hele events. De ingebouwde prijs is USD per miljoen events uit de openbare lijst. `--budget-month` telt die schatting niet mee.
- **Budget:** `--budget-month` toetst op de bytes na `--rules`. De regel `total` en de kosten daarvan tonen de bytes vóór de regels; de besparing staat apart. De schatting van geïndexeerde events hoort niet bij het budget.

Uitgewerkt voorbeeld (met een unit test): een steekproef van 1000000 bytes met `--window 1m` geeft factor 1440, dus 1440000000 bytes = 1.440 GB per dag. Met een ingest-prijs van 0.10 per GB is dat 0.144, getoond als 0.14 per dag, en 4.32 per maand (30 dagen).

## Vergelijking met alternatieven

Een eerlijke vergelijking met vijf producten. De pagina's zijn nagelezen op 8 oktober 2026. Wat daar niet staat, staat hier als niet geverifieerd.

**Log10x** (https://doc.log10x.com/faq/pricing/tiers-billing/) doet patroonanalyse en kostenreductie. De pagina zegt "Yes, and running the engine needs no account at all." Een engine zonder licentie "synthesizes a built-in evaluation license" en draait "up to 10 nodes, for 30 days from process start. It runs air-gapped". Boven de tabel staat "Pricing is per node, with unlimited log volume and all products included at every tier." De kolommen zijn Tier, Nodes, Monthly en Annual. Free staat er als "Free | up to 10 | $0 | $0", Starter als "Starter | up to 30 | $99 | $990". $99 per maand is de prijs van de hele Starter-tier tot 30 nodes, niet $99 per node. De binaries en de runtime zijn proprietary: op https://github.com/log-10x/pipeline-releases staat "Log10x binaries and runtime | Proprietary (commercial license required)" en "The installation scripts are open source", en het LICENSE-bestand heet "Log10x Proprietary Software License". Kies Log10x voor een product met een pipeline. Kies log-bill voor één open binary die een bestand leest en elke telling uitlegt.

**Cribl Stream** (https://cribl.io/pricing/stream/) is een pipeline. De pagina zegt "Route data from any source to any destination." Het gratis plan gaat tot 1 TB/dag. De creditprijzen op die pagina zijn 0.32 Credits / GB (Enterprise, Cloud Worker), 0.26 Credits / GB (Enterprise, Hybrid Worker) en 0.27 Credits / GB (Standard, Cloud Worker), met "1 credit = $1". Cribl routeert en samplet data echt. log-bill voert geen regels uit: het simuleert ze en verstuurt niets.

**Datadog Log Patterns** (https://docs.datadoghq.com/logs/explorer/analytics/patterns/) groepeert logs in de Log Explorer van Datadog. De pagina zegt "The pattern detection is based on 10,000 log samples." Of Patterns gratis is, is niet geverifieerd: die pagina noemt geen prijs. Het gaat over logs die je daar al bevraagt, niet over een bestand op schijf, en alleen binnen Datadog.

**Grafana Adaptive Logs** (https://grafana.com/docs/grafana-cloud/adaptive-telemetry/adaptive-logs/) "is a log optimization tool in Grafana Cloud". De pagina zegt dat het je logs laat droppen "before they reach Grafana Cloud". Het staat beschreven als onderdeel van Grafana Cloud.

**Grafana Logs Drilldown** (https://grafana.com/docs/grafana/latest/visualizations/simplified-exploration/logs/patterns/) toont patronen bij de logs. De pagina zegt "The Grafana Logs Drilldown app shows you the patterns alongside their log volumes." "Patterns are ephemeral and are only mined from the previous three hours of your logs." "To see patterns on the Patterns tab, you must have the pattern ingester enabled in your Loki configuration file." Het werkt op logs die al in Loki zitten, en alleen op de laatste drie uur.

**log-bill** is MIT OR Apache-2.0, één binary, geen account, geen netwerk, niet aan één leverancier gebonden. De prijzen komen uit het bestand dat je meegeeft, en de rekensom is per patroon te volgen.

## Beperkingen

- Geen officieel tarief van welke leverancier ook. Prijzen komen alleen uit je prijsbestand. Met `--prices` en `--window` eindigt de tekstuitvoer op `estimate: extrapolation of one sample with your own prices; not an official rate`.
- Geen contractkortingen, staffels of minimale afname. Retentie van geïndexeerde logs gebruikt de openbare lijstprijs, of het bedrag dat je met `--indexed-price` meegeeft, niet een eigen contract.
- Compressie en opslag verschillen per platform. log-bill telt de ruwe bytes van de regels in de steekproef. Een platform kan na parsing meten, metadata meetellen of per event rekenen.
- Een prijs per miljoen events is optioneel en staat los van de prijs per GB. Op https://docs.datadoghq.com/account_management/billing/pricing staat "Datadog charges for ingested logs based on the total number of gigabytes submitted to the Datadog Logs service." en "Datadog charges per million log events submitted for indexing at the rate designated for the retention policy you selected." De ingest-prijs per GB blijft in het prijsbestand. `--retention`, `--billing`, `--indexed-price` of `--index-rules` voegt de schatting van geïndexeerde events toe uit https://www.datadoghq.com/pricing/list/ (gelezen op 2026-10-14). Zonder die opties verandert de uitvoer niet. De tool rekent een aantal events niet om naar GB. `--budget-month` telt de schatting van geïndexeerde events niet mee. Flex Logs worden niet geprijsd.
- Een steekproef is één venster. Dag- en maandbedragen zijn een extrapolatie.
- Het weet niet of een regel nodig is voor security of compliance. Een drop-simulatie is geen advies om te droppen.
- Geen gzip-invoer in deze versie.
- Na een sleutelwoord (`secret`, `token`, `password`, `bearer` enzovoort) gevolgd door een spatie wordt het volgende woord altijd gemaskeerd, ook in gewone zinnen: `This is a secret mission report` → `This is a secret <SECRET> report`. Dat is bewust: liever maskeren dan lekken.
- Een tab is ook een scheiding. Geëscapete aanhalingstekens in een bericht verbergen de waarde nog steeds (`{\"password\":\"open sesame\"}`). Een waarde die met `[` of `{` begint wordt gemaskeerd tot het bijpassende haakje (`password=["alpha", "beta"]`). Een waarde die een sleutelwoord bevat (`password=secretPass1`) wordt helemaal gemaskeerd. Een e-mailadres slikt geen sleutelwoord op dat een geheim inleidt (`id=a@b.co.token=hunterpass`).
- Lookalike-tekens in een sleutelnaam worden ook herkend, zodat een sleutel die alleen op `password` of `secret` lijkt de waarde nog steeds verbergt. Fullwidth-letters worden ASCII, veelvoorkomende Cyrillische en Griekse lookalikes worden Latijn, en onzichtbare tekens in de sleutel worden genegeerd.
- Een regeleinde of een carriage return vóór `=` of `:` is een scheiding, zodat `password`, dan een newline, dan `= rosebud` de waarde verbergt. Een `\uXXXX`-escape in een sleutel is dat teken, zodat `pass\u0077ord` de waarde verbergt. Een geheim wordt herkend vóór een UUID-, IP- of e-mailmasker, zodat `password-123e4567-e89b-12d3-a456-426614174000=` en `password-10.0.0.1=` de waarde nog verbergen, en een e-mailadres als sleutel (`a@b.co=`) ook. `"a b"@example.com` is een e-mailadres. Een getal met een afsluitend `!`, `?`, `,` of `;`, zoals `7!`, is `<NUM>`; een afsluitende punt blijft staan (`1699482000.`). Staat `message`, `msg`, `log` of de naam van `--message-field` twee keer in de regel, dan geldt de latere waarde.
- Alleen gewone bestanden worden gelezen. Een FIFO, een symlink naar een FIFO, een map of een apparaat wordt geweigerd met exit 2, zonder op een schrijver te wachten.
- Invoer moet UTF-8 zijn. UTF-16 en UTF-32 (bijvoorbeeld van `Out-File` of `>` in Windows PowerShell 5.1), met of zonder byte order mark, worden geweigerd met exit 2 en een melding die zegt hoe je ze omzet: `iconv -f UTF-16LE -t UTF-8 in.log > out.log`, of in PowerShell `Get-Content in.log | Out-File -Encoding utf8 out.log`. Hun NUL-bytes zouden anders een sleutel als `password` zo opknippen dat de maskering hem mist. Zonder byte order mark wordt de codering herkend aan de NUL-bytes in de eerste 4096 bytes; UTF-16-tekst die grotendeels geen ASCII is, wordt mogelijk niet herkend. NUL-bytes in UTF-8-tekst worden vóór het maskeren weggehaald, zodat ook die geen sleutel kunnen verbergen; de bytes tellen wel mee. Een UTF-8-byte order mark aan het begin van een tekstbestand wordt overgeslagen en telt mee in de bytes van de eerste regel, zodat dezelfde regel met en zonder dat teken één patroon is. Stuurtekens in een logregel en in de valuta, de bron en de datum van een prijsbestand worden niet ruw geschreven. Tekstuitvoer toont ze als `\u00XX` (ESC is `\u001b`) en escaped bidi-stuurtekens op dezelfde manier (U+202E is `\u202e`). JSON houdt het teken (een stuurteken wordt door JSON ge-escaped; een bidi-teken blijft UTF-8), zodat de uitvoer geldige JSON blijft.
- Als het resultaat niet geschreven kan worden, is de exitcode 3 en de melding `could not write output`. `--help` en `--version` gebruiken diezelfde code als hun tekst niet weg kan.
- In `--json` toont `inputs.paths` elk pad als tekst. Een bestandsnaam die geen geldige UTF-8 is (kan op Linux) wordt wel gelezen, maar getoond met U+FFFD voor de ongeldige bytes.

## Privacy

log-bill werkt volledig offline: geen netwerk, geen API-key, geen telemetrie. Patronen houden de vaste woorden van een regel en vervangen tijdstempels, UUID's, IP-adressen, e-mailadressen (een gewoon adres, een gequote lokaal deel zoals `"john.doe"@example.com`, en letters buiten ASCII zoals `jöhn@exämple.de`), geheimen, hex, getallen (cijfers, ook met een `+` of `-` ervoor of een decimale breuk, zoals `-12.5`, `3.14` en `+31612345678`, en cijfers buiten ASCII zoals `١٢٣٤٥`) en andere variabele woorden. Een regel waar niets te vervangen is, zoals `disk full`, wordt helemaal getoond, met het patroon-id, het aantal regels, het aantal bytes en het aandeel. Met `--prices` worden ook de valuta, de bron en de datum uit het prijsbestand getoond. Een fout over een logbestand, een prijsbestand of een regelsbestand bevat het pad niet, noch de ruwe logregel, de regel uit het regelsbestand of de inhoud van het prijsbestand. Een fout over een index-regelsbestand bevat het pad niet en ook de regel zelf niet. Lookalike-tekens in een sleutelnaam worden ook herkend, zodat een sleutel die alleen op `password` of `secret` lijkt de waarde nog steeds verbergt. Lektests stoppen een nepgeheim in elk soort invoer en controleren dat geen enkele uitvoer het bevat. Alle fixtures zijn synthetisch.

## Steunen

log-bill is gratis te gebruiken. Bespaart het je geld, dan kun je het werk steunen via [Ko-fi](https://ko-fi.com/pepai).

## Licentie

Naar keuze onder een van deze licenties:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

Copyright 2026 PEPAI.

## Contact

PEPAI, pep_ai@icloud.com

Meld bugs en ideeën via [Issues](https://github.com/qwpp7g79y4-maker/log-bill/issues); meld beveiligingsproblemen privé zoals beschreven in [SECURITY.md](SECURITY.md).
