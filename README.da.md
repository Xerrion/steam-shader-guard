# Steam Shader Guard

Til Linux-spillere med NVIDIA-grafik, der gentagne gange ser Steam behandle shaders.
Programmet giver hvert spil en separat mappe til gemte shaders. Det kan også kopiere
genbrugelige data fra spillets eksisterende shadercache.

En **shadercache** indeholder kompilerede grafikprogrammer, som spillet kan genbruge.
Den nye Steam-genvej springer NVIDIA-shaderforbehandling over.

**Dette er en eksperimentel omgåelse, ikke en officiel rettelse fra Valve eller NVIDIA.**
Programmet lover ikke højere FPS eller at fjerne al shaderkompilering.
Nye effekter, spilopdateringer og driveropdateringer kan stadig kræve kompilering.

## Før du starter

- Brug den almindelige Linux-udgave af Steam med NVIDIA-grafik.
  Flatpak og Snap understøttes ikke.
- Den færdige programfil virker på x86-64 Linux. Du behøver ikke Rust.
- Kør som din normale bruger, **ikke med `sudo`**.
- Kopiering af eksisterende shaders kræver ekstra diskplads. Originalerne bevares.

Du skal både **installere programmet** og **sætte spillene op**.
Installation alene ændrer ingen spilindstillinger.

## Sæt et spil op

### 1. Installer programmet

Åbn en terminal, og kør:

```sh
curl -fsSL https://raw.githubusercontent.com/Xerrion/steam-shader-guard/main/scripts/install.sh | sh
```

Kommandoen henter den seneste stabile udgivelse, kontrollerer programfilens checksum
og installerer programmet. Kontrollen sikrer, at den hentede fil svarer til udgivelsen,
før den bliver kørt. Den tilføjer **Steam (Shader Guard)** i applikationsmenuen.
Den sætter ingen spil op, kopierer ingen shaders og starter ikke Steam.

Kommandoen henter og kører [dette installationsscript](https://github.com/Xerrion/steam-shader-guard/blob/main/scripts/install.sh).
Læs det først, hvis du vil kontrollere det før brug.
Du kan også følge [vejledningen til manuel download](README.md#manual-download).
Brug samme kommando til at installere en nyere udgivelse.
Filer, du selv ændrede, bliver ikke overskrevet.
Den separate programfil i udgivelsen hedder `steam-shader-guard`.

### 2. Find spillets ID

Luk Steam og alle kørende spil helt. Steam kan stadig køre, når du lukker vinduet.
Brug derfor Steams handling til at afslutte programmet.

Kør derefter:

```sh
~/.local/bin/steam-shader-guard doctor
```

Kommandoen viser en læsbar liste over dine spil. Find spillet og tallet i kolonnen
**Game ID**. Den ændrer ingen filer.

Eksemplerne bruger `2357570`, som er Overwatch.
**Erstat tallet med dit eget spils ID.**

### 3. Kopier eksisterende shaders, hvis du vil genbruge dem

**Dette trin er valgfrit.** Hvis spillet allerede har gemte NVIDIA-shaders:

```sh
~/.local/bin/steam-shader-guard recover 2357570 --apply
```

Kommandoen kontrollerer originalerne, laver en separat kopi og kontrollerer kopien.
Den viser, hvilken fil den undersøger eller kopierer. Originalerne ændres ikke.
Den sætter ikke spillet op.

Hvis `doctor` viser **not found** under **Steam shader folder**, spring dette trin over.
Uden en kopi opretter spillet en tom Shader Guard-cache og bygger den, mens du spiller.

Kopier før spillets **første start med Shader Guard**.
Programmet overskriver ikke en Shader Guard-mappe, der allerede findes.

### 4. Sæt spillet op

```sh
~/.local/bin/steam-shader-guard enable 2357570 --apply
```

Kommandoen ændrer spillets Steam-startindstillinger, så det bruger Shader Guard.
Den viser, om spillet blev sat op eller sprunget over.
Den kopierer ingen shaders og starter ikke spillet.
Hvis spillet allerede har Shader Guard-startindstillinger, bevares de.

Spil med andre startindstillinger bliver heller ikke ændret.
Se [vejledningen til eksisterende startindstillinger](README.md#existing-launch-options),
hvis dit spil bliver sprunget over.

### 5. Start Steam gennem den nye genvej

Åbn **Steam (Shader Guard)** i applikationsmenuen.
Start derefter spillet normalt i Steam.

Du kan også starte Steam fra terminalen:

```sh
~/.local/bin/steam-shader-guard steam
```

**Brug denne genvej, hver gang du starter Steam.**
Installation og spilopsætning gemmes, men forbehandlingsindstillingen gælder kun
den Steam-session, du starter på denne måde. Den gamle Steam-genvej ændres ikke.
Du skal både bruge den nye genvej og sætte spillet op for at bruge begge dele af løsningen.

## Flere spil

Gentag kopiering og opsætning for hvert spil, du vil bruge.
Du kan også sætte alle egnede installerede spil op på én gang:

```sh
~/.local/bin/steam-shader-guard enable --all --apply
```

Spil med eksisterende startindstillinger og støtteprogrammer som Proton bliver sprunget over.
Kommandoen kopierer ingen shaders. Kopier dem først, hvis du vil genbruge dem.
Spil, du installerer senere, skal sættes op separat.

## Hvad betyder `--apply`?

Det betyder **udfør denne kommandos ændringer**, ikke “aktiver alt”.

Uden `--apply` viser `install`, `recover`, `enable`, `disable` og `uninstall`
kun planen. Du behøver ikke se planen først.
`doctor` og `scan` undersøger kun filer. `steam` og `run` starter programmer med det samme.

## Stop med at bruge Shader Guard

Luk Steam og alle kørende spil helt først.
For at stoppe brugen for ét spil:

```sh
~/.local/bin/steam-shader-guard disable 2357570 --apply
```

For at fortryde programmets gemte spilændringer og fjerne programmet og genvejen:

```sh
~/.local/bin/steam-shader-guard uninstall --apply
```

**Ingen af kommandoerne sletter dine shaderfiler.**
Dine egne senere ændringer i startindstillingerne bevares.
Hvis ændrede startindstillinger eller en ændret genvej stadig bruger programmet,
stopper afinstallationen og forklarer, hvad du skal gøre.

## Hjælp og begrænsninger

Brug `~/.local/bin/steam-shader-guard --help` til kommandolisten.
Brug eksempelvis `enable --help` til hjælp om spilopsætning.
Kommandoerne viser, hvad de gør, hvad der ændrede sig, og næste trin.

Programmet kan ikke genskabe manglende shaderdata.
Det kopierer kun data, som det kan identificere sikkert.
Test på forskellige Linux-udgaver og NVIDIA-drivere er endnu begrænset.
Se [hvad vi testede](https://github.com/Xerrion/steam-shader-guard/blob/main/VALIDATION.md).

Selve programmet sender ingen rapporter og foretager ingen netværkskald.
Installationsscriptet henter udgivelsesfiler fra GitHub.
Steam bruger fortsat sine egne netværksforbindelser.

Læs [den fulde engelske vejledning](README.md) for flere konti, andre mapper,
JSON-rapporter til scripts og udviklervejledning. Normal kommandoudskrift er
læsbar tekst. `doctor`, `scan` og `recover` kan vise JSON med `--json`.
