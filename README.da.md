# Steam Shader Guard — kort dansk vejledning

Et Rust-program til at undersøge, reparere og beskytte NVIDIA-shadercache i
**almindelig Steam på Linux**. Flatpak og Snap understøttes ikke i første version.
Det er en lokal omgåelse af Steam-fejlen, ikke en officiel rettelse fra Valve.

1. Pak arkivet ud, og luk Steam og Wine-spil.
2. Kør `./steam-shader-guard doctor` for at finde dine spil.
3. Kør `./steam-shader-guard install --apply` for at installere programmet og
   tilføje **Steam (Shader Guard)** i applikationsmenuen.
4. For et berørt spil: kør `./steam-shader-guard scan APPID`, derefter
   `./steam-shader-guard recover APPID --apply` og
   `./steam-shader-guard enable APPID --apply`.
5. Start Steam via **Steam (Shader Guard)**, og åbn spillet normalt.

Erstat `APPID` med spillets Steam-nummer. Overwatch bruger `2357570`.
Reparer en eksisterende cache, før du starter spillet gennem profilen første gang.
Originalerne bliver bevaret; der skal være diskplads til en separat kopi.

Uden `--apply` viser ændringskommandoerne kun planen. `enable --all --apply`
tilknytter installerede spil med tomme startindstillinger; det reparerer ikke
deres cache automatisk. Spil med egne startindstillinger bliver sprunget over.
Nye spil skal tilknyttes senere.

Cache er stadig aktiv. Nye effekter, nye spil og driveropdateringer kan fortsat
kræve kompilering. Den gamle Steam-genvej kan omgå indstillingen til at springe
forbehandling over; brug den nye genvej.

Fortryd med `~/.local/bin/steam-shader-guard uninstall --apply`, mens Steam og
spil er lukket. Shaderdata bevares. Egne ændringer overskrives ikke.

Læs [den fulde vejledning](README.md) for flere konti, særlige cacheplaceringer,
eksisterende startindstillinger og begrænsninger. Programmet foretager ingen
netværkskald eller uploads på egen hånd.

GitHub Actions tester med Rust **1.99.0** på x86-64 Linux (GNU og musl).
Versionstags, der svarer præcist til `Cargo.toml`, udgiver automatisk et statisk
musl-arkiv efter beståede tests. Hent både arkivet og `SHA256SUMS` fra udgivelsen,
og kør `sha256sum --check SHA256SUMS`, før du pakker arkivet ud.
Se [bygge- og udgivelsesvejledningen](README.md#releases) for detaljer.
