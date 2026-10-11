# log-bill

[English](README.md) · Nederlands

Schat offline wat je logs per patroon kosten, voordat je ze verstuurt.

[Installatie](#installatie) • [Snel aan de slag](#snel-aan-de-slag) • [Hoe het rekent](#hoe-het-rekent) • [Alternatieven](#vergelijking-met-alternatieven) • [Beperkingen](#beperkingen) • [Referentie](docs/reference.nl.md)

log-bill is een opdrachtregelprogramma in Rust. Het leest een steekproef uit een logbestand op je eigen machine (platte tekst of JSON lines), groepeert de regels tot patronen door de variabele delen te maskeren, en laat per patroon zien hoeveel regels en bytes het is en welk aandeel van het volume. Met een tijdvenster en een prijsbestand dat je zelf meegeeft rekent het de steekproef om naar een geschatte prijs per dag en per maand. Het kan `drop`- en `sample`-regels simuleren, twee steekproeven vergelijken, elke rekensom uitleggen, JSON schrijven en een CI-job laten falen als een budget wordt overschreden. Het heeft geen account nodig en maakt geen netwerkverbinding.

De uitvoer van de tool is Engels; de voorbeelden hieronder tonen die precies zoals de binary hem schrijft.

## Waarom

Teams betalen per GB voor logs en zien vaak pas op de factuur welk patroon het volume maakt. Twee opmerkingen op Hacker News, nagelezen op 8 oktober 2026: nthypes schrijft "My bill jumped nearly 1000% before alerts caught it" (https://news.ycombinator.com/item?id=46034896) en mleonhard noemt "the risk of bad logging running up a $70,000/day bill with CloudWatch Logs" (https://news.ycombinator.com/item?id=45630239). log-bill laat vóór het versturen, op een gewoon bestand, zien welke patronen de bytes zijn en wat een drop- of sample-regel zou schelen.

## Installatie

Je hebt een Rust-toolchain nodig (Rust 2021 edition).

```sh
cargo install --path .
log-bill --help
```

Of bouw met `cargo build --release`; de binary staat dan in `target/release/log-bill`.

## Snel aan de slag

Dit is de echte uitvoer van de binary op de synthetische fixtures in `tests/fixtures`. Een test draait elk voorbeeld en vergelijkt de uitvoer.

Een logbestand groeperen tot patronen:

```console
$ log-bill scan tests/fixtures/scan-text.log
total: 3 lines, 118 bytes
Pe19dc70d  2 lines  108 bytes  91.5%  user <NUM> failed at <TS> from <IP>
P46ab1618  1 line  10 bytes  8.5%  disk full
```

Een steekproef van één minuut omrekenen naar een dag en een maand:

```console
$ log-bill scan --window 1m tests/fixtures/scan-text.log
total: 3 lines, 118 bytes
Pe19dc70d  2 lines  108 bytes  91.5%  user <NUM> failed at <TS> from <IP>
P46ab1618  1 line  10 bytes  8.5%  disk full
factor: 1440
per day: 169920 bytes, 0.000 GB
per month: 5097600 bytes, 0.005 GB
```

118 bytes in één minuut is × 1440 = 169920 bytes per dag, en × 30 = 5097600 bytes per maand. Met 1000000000 bytes per GB is dat 0.000 GB per dag en 0.005 GB per maand, op drie decimalen.

Een sample-regel simuleren:

```console
$ log-bill scan --rules tests/fixtures/sample-0.1.txt tests/fixtures/sample-zero.log
total: 1 line, 10 bytes
P139ac555  1 line  10 bytes  100.0%  xxxxxxxxx
after rules: 1 line, 10 bytes -> 0 lines, 1 byte, savings 1 line, 9 bytes
P139ac555  1 line, 10 bytes -> 0 lines, 1 byte, savings 1 line, 9 bytes, rule 1
```

`sample-zero.log` is negen letters x en een regeleinde: 1 regel, 10 bytes. `sample 0.1` houdt `floor(1 × 0.1) = 0` regels en `floor(10 × 0.1) = 1` byte over.

Twee steekproeven vergelijken:

```console
$ log-bill diff tests/fixtures/diff-old.log tests/fixtures/diff-new.log
total: 3 lines, 17 bytes -> 2 lines, 12 bytes, change -1 line, -5 bytes
new P229176bd  0 -> 6 bytes, change +6 bytes, share 0.0% -> 50.0%, change +50.0 percentage points  gamma
gone P7627619b  5 -> 0 bytes, change -5 bytes, share 29.4% -> 0.0%, change -29.4 percentage points  beta
P8ac625bb  12 -> 6 bytes, change -6 bytes, share 70.6% -> 50.0%, change -20.6 percentage points  alpha
new patterns: 6 of 12 bytes, 50.0%
```

De oude steekproef is twee keer `alpha` (12 bytes) en één keer `beta` (5 bytes): 17 bytes. De nieuwe is `alpha` (6) en `gamma` (6): 12 bytes. Het nieuwe patroon `gamma` is 6 van 12 bytes, 50.0%.

Prijzen komen alleen uit een JSON-bestand dat je meegeeft met `--prices` (samen met `--window`); zie [het prijsbestand](docs/reference.nl.md#prijsbestand). Elke tekstuitvoer met bedragen eindigt met `estimate: extrapolation of one sample with your own prices; not an official rate` ("schatting: extrapolatie van één steekproef met je eigen prijzen; geen officieel tarief"). Alle opties, exitcodes en foutmeldingen staan in de [referentie](docs/reference.nl.md) en in `log-bill --help`.

## Hoe het rekent

Elk getal is terug te voeren op deze regels. De volledige maskerregels staan in de [referentie](docs/reference.nl.md#maskering).

- **Bytes:** de ruwe bytes van een regel plus 1 voor het regeleinde. Lege regels tellen niet. Bij JSON lines de bytes van de hele regel.
- **Patroon:** het bericht met de variabele delen gemaskeerd (`<TS>`, `<UUID>`, `<IP>`, `<EMAIL>`, `<SECRET>`, `<HEX>`, `<NUM>`, `<VAR>`). Het id is `P` plus de eerste 8 hextekens van de 64-bit FNV-1a-hash van de patroontekst.
- **Aandeel:** bytes van het patroon gedeeld door de totale bytes, als procent met 1 decimaal, half van nul af afgerond.
- **Venster:** de factor per dag is 86400 gedeeld door het venster in seconden. Een maand is `--days-per-month` dagen, standaard 30.
- **Kosten:** per dag = bytes in de steekproef × factor / bytes per GB × prijs per GB, apart voor ingest en index. Per maand = per dag × dagen per maand. Er wordt gerekend met gehele getallen en breuken; afronden gebeurt pas bij het tonen: bedragen op 2 decimalen, GB op 3 decimalen, half van nul af.
- **Regels:** `drop <regex>` of `sample <fractie> <regex>`, getoetst op de ruwe regel, de eerste die past wint. Een gesampled patroon houdt de verwachte waarde, één keer naar beneden afgerond voor de regels en één keer voor de bytes, onafhankelijk van elkaar. Daardoor kan een patroon op 0 regels en toch nog wat bytes uitkomen. De regex en de ruwe regel worden nooit getoond.
- **Budget:** `--budget-month` toetst op de bytes na `--rules`. De regel `total` en de kosten daarvan tonen de bytes vóór de regels; de besparing staat apart.

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
- Geen contractkortingen, staffels, minimale afname of retentieprijzen.
- Compressie en opslag verschillen per platform. log-bill telt de ruwe bytes van de regels in de steekproef. Een platform kan na parsing meten, metadata meetellen of per event rekenen.
- Geen prijs per miljoen events, alleen per GB; log-bill rekent ook index per GB. Op https://docs.datadoghq.com/account_management/billing/pricing staat "Datadog charges for ingested logs based on the total number of gigabytes submitted to the Datadog Logs service." en "Datadog charges per million log events submitted for indexing at the rate designated for the retention policy you selected." De ingest-prijs per GB past in het prijsbestand. De indexprijs per miljoen events past daar niet in: de tool rekent een aantal events niet om naar GB. Laat de indexprijs weg, tenzij je zelf al een bedrag per GB hebt.
- Een steekproef is één venster. Dag- en maandbedragen zijn een extrapolatie.
- Het weet niet of een regel nodig is voor security of compliance. Een drop-simulatie is geen advies om te droppen.
- Geen gzip-invoer in deze versie.
- Na een sleutelwoord (`secret`, `token`, `password`, `bearer` enzovoort) gevolgd door een spatie wordt het volgende woord altijd gemaskeerd, ook in gewone zinnen: `This is a secret mission report` → `This is a secret <SECRET> report`. Dat is bewust: liever maskeren dan lekken.
- Alleen gewone bestanden worden gelezen. Een FIFO, een map of een apparaat wordt geweigerd met exit 2.

## Privacy

log-bill werkt volledig offline: geen netwerk, geen API-key, geen telemetrie. Geen uitvoer of foutmelding bevat een volledige ruwe logregel; alleen gemaskeerde patronen, aantallen en regelnummers. Lektests stoppen een nepgeheim in elk soort invoer en controleren dat geen enkele uitvoer het bevat. Alle fixtures zijn synthetisch.

## Licentie

Naar keuze onder een van deze licenties:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

Copyright 2026 PEPAI.

## Contact

PEPAI, pep_ai@icloud.com
