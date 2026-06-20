## Konsistenzregel für Qualität A

Qualitätsstufe A darf nur gesetzt werden, wenn der gesamte Index Eintrag
widerspruchsfrei ist.

Ein Eintrag ist nicht Qualität A, wenn ein Pflichtfeld dem A Status widerspricht.

Widersprüche blockieren Qualität A automatisch.

Typische Widersprüche sind:

Status ist `Blockiert`, aber Qualitätsstufe ist `A`.
Status ist `Compile fehlgeschlagen`, aber Qualitätsstufe ist `A`.
Status ist `Warnungen vorhanden`, aber Qualitätsstufe ist `A`.
`Kompiliert ohne Fehler` ist `Nein`, aber Qualitätsstufe ist `A`.
`Kompiliert ohne Warnungen` ist `Nein`, aber Qualitätsstufe ist `A`.
`Tests oder Checks bestanden` ist `Nein`, aber Qualitätsstufe ist `A`.
`Direkt integriert` ist `Nein`, aber Qualitätsstufe ist `A`.
`Vollständig übersetzt` ist `Nein`, aber Qualitätsstufe ist `A`.
`Ein zu eins Parität geprüft` ist `Nein`, aber Qualitätsstufe ist `A`.
`Nachbeauftragung nötig` ist `Ja`, aber Qualitätsstufe ist `A`.
Ein Blocker ist vorhanden, aber Qualitätsstufe ist `A`.
Review ist erforderlich, aber nicht bestanden.
Ein ungültiger Statuswert wird verwendet.
Ein Pflichtfeld fehlt.
Ein Eintrag enthält doppelte oder widersprüchliche Felder.

Wenn ein Widerspruch vorhanden ist, gilt der niedrigste belegbare Zustand.

Qualität A darf dann nicht gesetzt werden.

Der Eintrag muss auf den realen Status zurückgesetzt werden.

Beispiele:

Wenn Build Fehler vorhanden sind, ist die Datei nicht A.
Wenn Warnungen vorhanden sind, ist die Datei nicht A.
Wenn ein Blocker vorhanden ist, ist die Datei nicht A.
Wenn Nachbeauftragung nötig ist, ist die Datei nicht A.
Wenn Review noch fehlt, ist die Datei nicht final A.
Wenn Parität nicht geprüft wurde, ist die Datei nicht A.

Qualität A ist nur erlaubt, wenn alle Pflichtfelder konsistent `Ja` oder
widerspruchsfrei erfüllt sind.

Der Index SoT darf keinen A Eintrag enthalten, der sich durch eigene Felder
selbst widerlegt.
