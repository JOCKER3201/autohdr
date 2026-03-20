Hello!

This is my project RTX hdr on linux

NOTE: I wrote this project using gemini cli, but actually I didn't write anything in the code because I only issued commands,
this code was written for me because I needed autohdr for some games,
I don't want AI slop to become popular, so if you end up here, don't spread it around,
Besides, I don't know if AI hasn't hidden some crap here, so be careful,
you use at your own risk.

1. env

   ⚙️ Logika Działania
   * AUTOHDR_ENABLE
       * Co robi: Główny przełącznik warstwy.
       * Wartości: 1 (aktywna), 0 (całkowicie wyłączona).
       * Opis: Pozwala na szybkie wyłączenie efektu HDR bez konieczności usuwania warstwy z konfiguracji Vulkana czy Steam. Jeśli ustawisz 0, warstwa będzie jedynie przekazywać polecenia do sterownika, nie modyfikując obrazu.


  ☀️ Jasność (Luminancja)
   * AUTOHDR_MAX_LUMINANCE
       * Co robi: Ustawia "szczyt" jasności (Peak Brightness) w nitach
       * Opis: Decyduje, jak bardzo "razić" mają najjaśniejsze elementy (słońce, eksplozje, światła odblaskowe). Powinieneś ustawić tu wartość odpowiadającą certyfikatowi Twojego monitora (np. 400, 600, 1000).


   * AUTOHDR_MID_LUMINANCE
       * Co robi: Ustala jasność bazową dla typowych scen.
       * Opis: Decyduje o ogólnym naświetleniu gry. Wyższa wartość sprawia, że gra w dzień wygląda na jaśniejszą i bardziej "świetlistą". Zbyt wysoka wartość może sprawić, że obraz będzie wyglądał na prześwietlony.


   * AUTOHDR_MIN_LUMINANCE
       * Co robi: Mnożnik jasności cieni i ciemnych obszarów (Black Point).
       * Opis: Służy do pogłębiania czerni. 
           * 1.0 to standardowe zachowanie.
           * Wartości niższe (np. 0.1) przyciemniają noc i cienie (kluczowe dla Minecrafta, aby uniknąć "szarej" nocy).
           * Wartości wyższe rozjaśniają detale w ciemności.


  🎨 Kolor i Intensywność
   * AUTOHDR_VIBRANCE
       * Co robi: Inteligentne, nieliniowe nasycenie barw.
       * Opis: Najważniejsza zmienna dla kolorów. Wyszukuje piksele o niskim nasyceniu (wyblakłe) i wzmacnia je, pozostawiając kolory już nasycone w spokoju. Dzięki temu świat gry staje się żywy, ale nie wygląda nienaturalnie (np. twarze nie stają się marchewkowe).


   * AUTOHDR_SATURATION
       * Co robi: Klasyczne, liniowe nasycenie.
       * Opis: Mnoży nasycenie każdego piksela o tę samą wartość. Zwykle zaleca się pozostawienie tej wartości na 1.0 i korzystanie z AUTOHDR_VIBRANCE dla lepszego efektu wizualnego.

   * AUTOHDR_INTENSITY
       * Co robi: Kontroluje intensywność całego efektu HDR.
       * Opis: Pozwala na płynne przejście między obrazem SDR a pełnym HDR (0.0 - obraz SDR, 1.0 - pełny efekt). Domyślnie: 1.0.

   * AUTOHDR_BLACK_LEVEL
       * Co robi: Reguluje poziom czerni (tylko najciemniejsze obszary).
       * Opis: Wartości dodatnie rozjaśniają najgłębszą czerń, wartości ujemne ją pogłębiają. Nie wpływa na średnie tony ani światła. Domyślnie: 0.0.
