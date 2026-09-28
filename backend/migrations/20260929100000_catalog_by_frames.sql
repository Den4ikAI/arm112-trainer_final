-- Справочники служб и типов происшествий — ровно как в боевой системе.
--
-- ИСТОЧНИК: материалы экспертов из чата задачи (21.09.2026):
--   refs/СЛУЖБЫ 112.docx   — окно «Добавьте службы», пролистанное целиком
--                            (image1…image55);
--   refs/КАРТОЧКА 112.docx — выпадающий список «что случилось?»
--                            (image2…image13).
-- Выписки дословно, с номером кадра: backend/docs/справочники/*.txt;
-- этот файл собран скриптом собрать-миграцию.py оттуда же.
--
-- Службы: 211 на кадрах, из них 172 у нас не было (районные ДДС,
-- префектуры, ОГДЦ, АСУ НС, «112 Мос. обл.», окружные ГБУ АД …). Наши
-- службы, которых в окне нет (20, столбцы классификатора вроде
-- «Территориальные ОИВ»), не удаляются — на них ссылаются карточки и
-- матрица оповещения, — а скрываются: их нет в service_catalog.
--
-- Скрытая служба не попадает ни в автоподбор, ни в эталон разбора
-- (domain/classify.rs): иначе курсант не смог бы её добавить, а разбор
-- штрафовал бы за её отсутствие. Службы только с кадров — без столбца
-- в классификаторе — автоматически не подбираются и в эталон не входят:
-- их добавляют вручную.
--
-- Между image3 и image4 часть списка в кадр не попала — см.
-- OPEN-QUESTIONS.md, вопрос 23.

-- Порядок окна «Добавьте службы»: позиция — как при прокрутке сверху вниз
CREATE TABLE service_catalog (
    service_id  text PRIMARY KEY REFERENCES services(id),
    position    int  NOT NULL UNIQUE,
    -- Как на плашке в полосе оповещения: часть до скобки
    short_title text NOT NULL,
    -- Номер кадра refs/СЛУЖБЫ 112/imageN
    frame       int  NOT NULL
);

-- Все службы окна — в справочник служб. Название — дословно с кадра.
-- Уже заведённые не трогаем (ON CONFLICT DO NOTHING): их названия сняты
-- с тех же кадров (20260928153000_service_names.sql). На чистом
-- развёртывании миграции идут раньше импорта классификатора, и без этой
-- вставки ссылка из service_catalog упала бы; импорт потом лишь
-- допишет заголовок столбца (source_title)
INSERT INTO services (id, title) VALUES
  ('mchs', 'Служба 101 (ГУ МЧС России по г.Москве , ГКУ "Пожарно спасательный центр" ОДС)'),
  ('fsb', 'ФСБ'),
  ('cemp', 'ЦЭМП'),
  ('smp', 'Служба 103 (ГБУ города Москвы Станция скорой и неотложной медицинской помощи им.А.С. Пучкова)'),
  ('mosgaz', 'Служба 104 (АО "МОСГАЗ" Диспетчерское управление)'),
  ('ogdc', 'ОГДЦ'),
  ('codd', 'ЦОДД (ГКУ "Центр организации дорожного движения")'),
  ('gormost', 'Гормост (Гормост)'),
  ('mosgortrans', 'Мосгортранс'),
  ('avtomobil-nye-dorogi', 'Автодороги'),
  ('mosvodokanal', 'Мосводоканал (АО "Мосводоканал")'),
  ('moesk-pao-rosseti-moskovskij-region', 'Россети МР'),
  ('moek', 'МОЭК'),
  ('dep-zhkh', 'Деп. ЖКХ (Департамент ЖКХ)'),
  ('metro', 'Метро'),
  ('asu-ns', 'АСУ НС (Автоматизированная система управления …)'),
  ('departament-rbipk-gku-mosbez', 'Мос.Без. (Московская Безопасность)'),
  ('112-mos-obl', '112 Мос. обл. (112 Московской области)'),
  ('rsvo', 'ФГУП РСВО (Российские сети вещания и оповещения)'),
  ('oek', 'ОЭК (Объединенная энергетическая компания)'),
  ('moslift', 'Мослифт (Лифт МСК)'),
  ('mosvodostok', 'Мосводосток (Мосводосток)'),
  ('moskollektor', 'Москоллектор (Москоллектор)'),
  ('voennaya-komendatura', 'Воен. комендатура (Воен. комендатура)'),
  ('poselenie-chertanovo-severnoe', 'Поселение Чертаново Северное (ДДС района Чертаново Северное города Москвы)'),
  ('poselenie-akademicheskij', 'Поселение Академический (ДДС Академического района города Москвы)'),
  ('poselenie-lomonosovskij', 'Поселение Ломоносовский (ДДС Ломоносовского района города Москвы)'),
  ('poselenie-bibirevo', 'Поселение Бибирево (ДДС района Бибирево города Москвы)'),
  ('poselenie-yuvao', 'Поселение ЮВАО (ДДС префектуры Юго-Восточного административного округа города Москвы)'),
  ('poselenie-vao', 'Поселение ВАО (ДДС префектуры Восточного административного округа города Москвы)'),
  ('poselenie-zyablikovo', 'Поселение Зябликово (ДДС района Зябликово города Москвы)'),
  ('poselenie-silino', 'Поселение Силино (ДДС района Силино города Москвы)'),
  ('oati', 'ОАТИ (Объединение Административно-Технических Инспекций города Москвы)'),
  ('msppn', 'ГБУ МСППН (Московская служба психологической помощи населению ГБУ города Москвы)'),
  ('upr-rajona-veshnyaki', 'Упр. района Вешняки (ДДС района Вешняки города Москвы)'),
  ('poselenie-kryukovo', 'Поселение Крюково (ДДС района Крюково города Москвы)'),
  ('poselenie-matushkino', 'Поселение Матушкино (ДДС района Матушкино города Москвы)'),
  ('poselenie-novogireevo', 'Поселение Новогиреево (ДДС района Новогиреево города Москвы)'),
  ('poselenie-savelki', 'Поселение Савелки (ДДС района Савелки города Москвы)'),
  ('poselenie-staroe-kryukovo', 'Поселение Старое Крюково (ДДС района Старое Крюково города Москвы)'),
  ('poselenie-chertanovo-yuzhnoe', 'Поселение Чертаново Южное (ДДС района Чертаново Южное города Москвы)'),
  ('poselenie-teplyj-stan', 'Поселение Теплый Стан (ДДС района Теплый Стан города Москвы)'),
  ('poselenie-ryazanskij', 'Поселение Рязанский (ДДС Рязанского района города Москвы)'),
  ('poselenie-zelao', 'Поселение ЗелАО (ДДС префектуры Зеленоградского административного округа города Москвы)'),
  ('poselenie-pechatniki', 'Поселение Печатники (ДДС района Печатники города Москвы)'),
  ('poselenie-sao', 'Поселение САО (ДДС префектуры Северного административного округа города Москвы)'),
  ('poselenie-novokosino', 'Поселение Новокосино (ДДС района Новокосино города Москвы)'),
  ('poselenie-caricyno', 'Поселение Царицыно (ДДС района Царицыно города Москвы)'),
  ('poselenie-mar-ino', 'Поселение Марьино (ДДС района Марьино города Москвы)'),
  ('poselenie-kuz-minki', 'Поселение Кузьминки (ДДС района Кузьминки города Москвы)'),
  ('poselenie-mitino', 'Поселение Митино (ДДС района Митино города Москвы)'),
  ('poselenie-savelovskij', 'Поселение Савеловский (ДДС Савеловского района города Москвы)'),
  ('poselenie-schukino', 'Поселение Щукино (ДДС района Щукино города Москвы)'),
  ('poselenie-pokrovskoe-streshnevo', 'Поселение Покровское-Стрешнево (ДДС района Покровское-Стрешнево города Москвы)'),
  ('poselenie-pervomajskoe', 'Поселение Первомайское (ДДС поселения Первомайское в городе Москве)'),
  ('rzhd-moskovskayazhd', 'МЖД (Московская железная дорога)'),
  ('112-kal-obl', '112 Кал. обл. (112 Калужская область)'),
  ('poselenie-severnoe-tushino', 'Поселение Северное Тушино (ДДС района Северное Тушино города Москвы)'),
  ('poselenie-nagatinskij-zaton', 'Поселение Нагатинский Затон (ДДС района Нагатинский Затон города Москвы)'),
  ('poselenie-dorogomilovo', 'Поселение Дорогомилово (ДДС района Дорогомилово города Москвы)'),
  ('poselenie-koptevo', 'Поселение Коптево (ДДС района Коптево города Москвы)'),
  ('poselenie-krylatskoe', 'Поселение Крылатское (ДДС района Крылатское города Москвы)'),
  ('poselenie-tverskoj', 'Поселение Тверской (ДДС Тверского района города Москвы)'),
  ('poselenie-vnukovo', 'Поселение Внуково (ДДС района Внуково города Москвы)'),
  ('poselenie-yaroslavskij', 'Поселение Ярославский (ДДС Ярославского района города Москвы)'),
  ('poselenie-yuao', 'Поселение ЮАО (ДДС префектуры Южного административного округа города Москвы)'),
  ('poselenie-mihajlovo-yarcevskoe', 'Поселение Михайлово-Ярцевское (ДДС поселения Михайлово-Ярцевское города Москвы)'),
  ('poselenie-nekrasovka', 'Поселение Некрасовка (ДДС района Некрасовка города Москвы)'),
  ('poselenie-vyhino-zhulebino', 'Поселение Выхино-Жулебино (ДДС района Выхино-Жулебино города Москвы)'),
  ('poselenie-moskvorech-e-saburovo', 'Поселение Москворечье-Сабурово (ДДС района Москворечье-Сабурово города Москвы)'),
  ('poselenie-yuzao', 'Поселение ЮЗАО (ДДС префектура Юго-Западного административного округа города Москвы)'),
  ('poselenie-donskoj', 'Поселение Донской (ДДС Донского района города Москвы)'),
  ('poselenie-kurkino', 'Поселение Куркино (ДДС района Куркино города Москвы)'),
  ('poselenie-yuzhnoe-butovo', 'Поселение Южное Бутово (ДДС района Южное Бутово города Москвы)'),
  ('poselenie-lianozovo', 'Поселение Лианозово (ДДС района Лианозово города Москвы)'),
  ('departament-obrazovaniya', 'Деп. Обр. (Московский Департамент Образования)'),
  ('poselenie-lefortovo', 'Поселение Лефортово (ДДС района Лефортово города Москвы)'),
  ('poselenie-zamoskvorech-e', 'Поселение Замоскворечье (ДДС района Замоскворечье города Москвы)'),
  ('poselenie-svao', 'Поселение СВАО (ДДС префектуры Северо-Восточного административного округа города Москвы)'),
  ('poselenie-sokol-niki', 'Поселение Сокольники (ДДС района Сокольники города Москвы)'),
  ('poselenie-izmajlovo', 'Поселение Измайлово (ДДС района Измайлово города Москвы)'),
  ('poselenie-bogorodskoe', 'Поселение Богородское (ДДС района Богородское города Москвы)'),
  ('poselenie-preobrazhenskij', 'Поселение Преображенский (ДДС района Преображенское города Москвы)'),
  ('poselenie-vostochnoe-izmajlovo', 'Поселение Восточное Измайлово (ДДС района Восточное Измайлово города Москвы)'),
  ('poselenie-rostokino', 'Поселение Ростокино (ДДС района Ростокино города Москвы)'),
  ('poselenie-sokolinaya-gora', 'Поселение Соколиная гора (ДДС района Соколиная гора города Москвы)'),
  ('poselenie-horoshevo-mnevniki', 'Поселение Хорошево-Мневники (ДДС района Хорошево-Мневники города Москвы)'),
  ('poselenie-gol-yanovo', 'Поселение Гольяново (ДДС района Гольяново города Москвы)'),
  ('poselenie-severnoe-izmajlovo', 'Поселение Северное Измайлово (ДДС района Северное Измайлово города Москвы)'),
  ('poselenie-marfino', 'Поселение Марфино (ДДС района Марфино города Москвы)'),
  ('poselenie-metrogorodok', 'Поселение Метрогородок (ДДС района Метрогородок города Москвы)'),
  ('poselenie-fili-davydkovo', 'Поселение Фили-Давыдково (ДДС района Фили-Давыдково города Москвы)'),
  ('poselenie-yuzhnoe-medvedkovo', 'Поселение Южное Медведково (ДДС района Южное Медведково города Москвы)'),
  ('poselenie-ryazanovskoe', 'Поселение Рязановское (ДДС поселения Рязановское в городе Москве)'),
  ('departament-ppioos', 'Деп. природопользования (Департамент природопользования и охраны окружающей среды города Москвы)'),
  ('poselenie-timiryazevskij', 'Поселение Тимирязевский (ДДС Тимирязевского района города Москвы)'),
  ('poselenie-biryulevo-zapadnoe', 'Поселение Бирюлево Западное (ДДС района Бирюлево Западное города Москвы)'),
  ('poselenie-ivanovskij', 'Поселение Ивановский (ДДС Ивановского района города Москвы)'),
  ('poselenie-novo-peredelkino', 'Поселение Ново-Переделкино (ДДС района Ново-Переделкино города Москвы)'),
  ('poselenie-vostochnoe-degunino', 'Поселение Восточное Дегунино (ДДС района Восточное Дегунино города Москвы)'),
  ('poselenie-hovrino', 'Поселение Ховрино (ДДС района Ховрино города Москвы)'),
  ('poselenie-golovinskij', 'Поселение Головинский (ДДС Головинского района города Москвы)'),
  ('poselenie-beskudnikovo', 'Поселение Бескудниково (ДДС Бескудниковского района города Москвы)'),
  ('poselenie-levoberezhnyj', 'Поселение Левобережный (ДДС района Левобережный города Москвы)'),
  ('poselenie-dmitrovskij', 'Поселение Дмитровский (ДДС Дмитровского района города Москвы)'),
  ('poselenie-sokol', 'Поселение Сокол (ДДС района Сокол города Москвы)'),
  ('poselenie-molzhaninovskij', 'Поселение Молжаниновский (ДДС Молжаниновского района города Москвы)'),
  ('poselenie-hamovniki', 'Поселение Хамовники (ДДС района Хамовники города Москвы)'),
  ('poselenie-basmannyj', 'Поселение Басманный (ДДС Басманного района города Москвы)'),
  ('poselenie-yakimanka', 'Поселение Якиманка (ДДС района Якиманка города Москвы)'),
  ('poselenie-meschanskij', 'Поселение Мещанский (ДДС Мещанского района города Москвы)'),
  ('poselenie-presnenskij', 'Поселение Пресненский (ДДС Пресненского района города Москвы)'),
  ('mvd', 'Служба 102 (Дежурная часть ГУ МВД России по г.Москве)'),
  ('poselenie-mosrentgen', 'Поселение Мосрентген (ДДС поселения «Мосрентген» в городе Москве)'),
  ('poselenie-yasenevo', 'Поселение Ясенево (ДДС района Ясенево города Москвы)'),
  ('mgts', 'МГТС (Московская городская телефонная сеть)'),
  ('poselenie-scherbinka', 'Поселение Щербинка (ДДС городского округа Щербинка города Москвы)'),
  ('poselenie-kapotnya', 'Поселение Капотня (ДДС района Капотня города Москвы)'),
  ('poselenie-troick', 'Поселение Троицк (ДДС городского округа Троицк города Москвы)'),
  ('poselenie-nagatino-sadovniki', 'Поселение Нагатино-Садовники (ДДС района Нагатино Садовники)'),
  ('poselenie-klenovskoe', 'Поселение Клёновское (ДДС поселения Клёновское в городе Москве)'),
  ('poselenie-nagornyj', 'Поселение Нагорный (ДДС района Нагорный города Москвы)'),
  ('poselenie-babushkinskij', 'Поселение Бабушкинский (ДДС Бабушкинского района города Москвы)'),
  ('poselenie-butyrskij', 'Поселение Бутырский (ДДС Бутырского района города Москвы)'),
  ('poselenie-losinoostrovskij', 'Поселение Лосиноостровский (ДДС Лосиноостровского района города Москвы)'),
  ('poselenie-ostankinskij', 'Поселение Останкинский (ДДС Останкинского района города Москвы)'),
  ('poselenie-otradnoe', 'Поселение Отрадное (ДДС района Отрадное города Москвы)'),
  ('poselenie-sviblovo', 'Поселение Свиблово (ДДС района Свиблово города Москвы)'),
  ('poselenie-severnoe-medvedkovo', 'Поселение Северное Медведково (ДДС района Северное Медведково города Москвы)'),
  ('poselenie-severnyj', 'Поселение Северный (ДДС района Северный города Москвы)'),
  ('poselenie-orehovo-borisovo-severnoe', 'Поселение Орехово-Борисово Северное (ДДС Орехово-Борисово Северное города Москвы)'),
  ('poselenie-perovo', 'Поселение Перово (ДДС района Перово города Москвы)'),
  ('poselenie-altuf-evskij', 'Поселение Алтуфьевский (ДДС Алтуфьевского района города Москвы)'),
  ('poselenie-mozhajskij', 'Поселение Можайский (ДДС Можайского района города Москвы)'),
  ('poselenie-ramenki', 'Поселение Раменки (ДДС района Раменки города Москвы)'),
  ('poselenie-alekseevskij', 'Поселение Алексеевский (ДДС Алексеевского района города Москвы)'),
  ('poselenie-kuncevo', 'Поселение Кунцево (ДДС района Кунцево города Москвы)'),
  ('poselenie-troparevo-nikulino', 'Поселение Тропарево-Никулино (ДДС района Тропарево-Никулино города Москвы)'),
  ('poselenie-vostochnyj', 'Поселение Восточный (ДДС района Восточный города Москвы)'),
  ('poselenie-aeroport', 'Поселение Аэропорт (ДДС района Аэропорт города Москвы)'),
  ('poselenie-zapadnoe-degunino', 'Поселение Западное Дегунино (ДДС района Западное Дегунино города Москвы)'),
  ('poselenie-vojkovskij', 'Поселение Войковский (ДДС Войковского района города Москвы)'),
  ('poselenie-horoshevskij', 'Поселение Хорошевский (ДДС Хорошевского района города Москвы)'),
  ('poselenie-begovoj', 'Поселение Беговой (ДДС района Беговой города Москвы)'),
  ('poselenie-tekstil-schiki', 'Поселение Текстильщики (ДДС района Текстильщики города Москвы)'),
  ('poselenie-moskovskoe', 'Поселение Московское (ДДС поселения Московский в городе Москве)'),
  ('poselenie-brateevo', 'Поселение Братеево (ДДС района Братеево города Москвы)'),
  ('poselenie-filimonkovskoe', 'Поселение Филимонковское (ДДС поселения Филимонковское в городе Москве)'),
  ('kanal-imeni-moskvy', 'Канал им. Москвы (ФГБУ "Канал имени Москвы")'),
  ('poselenie-lyublino', 'Поселение Люблино (ДДС района Люблино города Москвы)'),
  ('poselenie-marushkinskoe', 'Поселение Марушкинское (ДДС поселения Марушкинское в городе Москве)'),
  ('od-departament-tszn', 'Деп. труда и соц.защиты (Департамент труда и социальной защиты населения города Москвы)'),
  ('poselenie-orehovo-borisovo-yuzhnoe', 'Поселение Орехово-Борисово Южное (ДДС района Орехово-Борисово Южное города Москвы)'),
  ('poselenie-danilovskij', 'Поселение Даниловский (ДДС Даниловского района города Москвы)'),
  ('poselenie-rogovskoe', 'Поселение Роговское (ДДС поселения Роговское в городе Москве)'),
  ('poselenie-zao', 'Поселение ЗАО (ДДС префектуры Западного административного округа города Москвы)'),
  ('poselenie-nizhegorodskij', 'Поселение Нижегородский (ДДС Нижегородского района города Москвы)'),
  ('poselenie-schapovskoe', 'Поселение Щаповское (ДДС поселения Щаповское в городе Москве)'),
  ('poselenie-yuzhnoportovyj', 'Поселение Южнопортовый (ДДС Южнопортового района города Москвы)'),
  ('evazhd', 'ЭВАЖД (ГБУ "Учреждение по эксплуатации высотных административных и жилых домов")'),
  ('poselenie-cao', 'Поселение ЦАО (ДДС префектуры Центрального административного округа)'),
  ('poselenie-cheremushki', 'Поселение Черемушки (ДДС района Черемушки города Москвы)'),
  ('poselenie-mar-ina-roscha', 'Поселение Марьина роща (ДДС района Марьина роща города Москвы)'),
  ('poselenie-krasnosel-skij', 'Поселение Красносельский (ДДС Красносельского района города Москвы)'),
  ('poselenie-strogino', 'Поселение Строгино (ДДС района Строгино города Москвы)'),
  ('poselenie-szao', 'Поселение СЗАО (ДДС префектуры Северо-Западного административного округа города Москвы)'),
  ('poselenie-biryulevo-vostochnoe', 'Поселение Бирюлево Восточное (ДДС района Бирюлево Восточное города Москвы)'),
  ('poselenie-chertanovo-central-noe', 'Поселение Чертаново Центральное (ДДС района Чертаново Центральное города Москвы)'),
  ('poselenie-gagarinskij', 'Поселение Гагаринский (ДДС Гагаринского района города Москвы)'),
  ('poselenie-obruchevskij', 'Поселение Обручевский (ДДС Обручевского района города Москвы)'),
  ('poselenie-zyuzino', 'Поселение Зюзино (ДДС района Зюзино города Москвы)'),
  ('poselenie-kon-kovo', 'Поселение Коньково (ДДС района Коньково города Москвы)'),
  ('poselenie-kotlovka', 'Поселение Котловка (ДДС района Котловка города Москвы)'),
  ('poselenie-severnoe-butovo', 'Поселение Северное Бутово (ДДС района Северное Бутово города Москвы)'),
  ('poselenie-kosino-uhtomskij', 'Поселение Косино-Ухтомский (ДДС района Косино-Ухтомский города Москвы)'),
  ('poselenie-solncevo', 'Поселение Солнцево (ДДС района Солнцево города Москвы)'),
  ('poselenie-taganskij', 'Поселение Таганский (ДДС района Таганский города Москвы)'),
  ('poselenie-ochakovo-matveevskoe', 'Поселение Очаково-Матвеевское (ДДС района Очаково-Матвеевское города Москвы)'),
  ('poselenie-yuzhnoe-tushino', 'Поселение Южное Тушино (ДДС района Южное Тушино города Москвы)'),
  ('poselenie-filevskij-park', 'Поселение Филевский парк (ДДС района Филевский парк города Москвы)'),
  ('poselenie-tinao', 'Поселение ТиНАО (ДДС префектуры Троицкого и Новомосковского округов города Москвы)'),
  ('poselenie-prospekt-vernadskogo', 'Поселение Проспект Вернадского (ДДС района Проспект Вернадского города Москвы)'),
  ('poselenie-desenovskoe', 'Поселение Десеновское (ДДС поселения Десеновское в городе Москве)'),
  ('poselenie-arbat', 'Поселение Арбат (ДДС района Арбат города Москвы)'),
  ('dezhurno-dispetcherskaya-sluzhba', 'Дежурно-диспетчерская служба (Дежурно-диспетчерская служба)'),
  ('poselenie-sosenskoe', 'Поселение Сосенское (ДДС поселения Сосенское в городе Москве)'),
  ('poselenie-kokoshkino', 'Поселение Кокошкино (ДДС поселения Кокошкино в городе Москве)'),
  ('centrregionvodhoz-moskovsko-okskoe-bvu', 'Центррегионводхоз (Центррегионводхоз)'),
  ('poselenie-voskresenskoe', 'Поселение Воскресенское (ДДС поселения Воскресенское в городе Москве)'),
  ('poselenie-krasnopahorskoe', 'Поселение Краснопахорское (ДДС поселения Краснопахорское в городе Москве)'),
  ('poselenie-voronovskoe', 'Поселение Вороновское (ДДС поселения Вороновское в городе Москве)'),
  ('mosoblgaz', 'Мособлгаз (Мособлгаз)'),
  ('poselenie-vnukovskoe', 'Поселение Внуковское (ДДС поселения Внуковское города Москвы)'),
  ('poselenie-novofedorovskoe', 'Поселение Новофедоровское (ДДС поселения Новофедоровское города Москвы)'),
  ('poselenie-kievskij', 'Поселение Киевский (ДДС поселения Киевский города Москвы)'),
  ('gbu-ad-vao', 'ГБУ АД ВАО (ГБУ Автодороги ВАО)'),
  ('gbu-ad-svao', 'ГБУ АД СВАО (ГБУ Автодороги СВАО)'),
  ('gbu-ad-yuvao', 'ГБУ АД ЮВАО (ГБУ Автодороги ЮВАО)'),
  ('gbu-ad-zelao', 'ГБУ АД ЗелАО (ГБУ Автодороги ЗелАО)'),
  ('gbu-ad-yuzao', 'ГБУ АД ЮЗАО (ГБУ Автодороги ЮЗАО)'),
  ('gbu-ad-yuao', 'ГБУ АД ЮАО (ГБУ Автодороги ЮАО)'),
  ('gbu-ad-sao', 'ГБУ АД САО (ГБУ Автодороги САО)'),
  ('gbu-ad-szao', 'ГБУ АД СЗАО (ГБУ Автодороги СЗАО)'),
  ('gbu-ad-cao', 'ГБУ АД ЦАО (ГБУ Автодороги ЦАО)'),
  ('gbu-ad-zao', 'ГБУ АД ЗАО (ГБУ Автодороги ЗАО)'),
  ('departament-kul-tury', 'Департамент культуры города Москвы (Департамент культуры города Москвы)'),
  ('gku-csa-imeni-e-p-glinki', 'ГКУ ЦСА (Центр социальной помощи)'),
  ('komitet-po-turizmu-g-moskvy', 'Мостуризм (Комитет по туризму города Москвы)'),
  ('departament-stroitel-stva-goroda-moskvy', 'Департамент строительства (Департамент строительства)'),
  ('komitet-veterinarii', 'Комитет ветеринарии (Комитет ветеринарии города Москвы)'),
  ('moszhilinspekciya', 'Мосжилинспекция (Государственная жилищная инспекция города Москвы)')
ON CONFLICT (id) DO NOTHING;

INSERT INTO service_catalog (service_id, position, short_title, frame) VALUES
  ('mchs', 1, 'Служба 101', 1),
  ('fsb', 2, 'ФСБ', 1),
  ('cemp', 3, 'ЦЭМП', 1),
  ('smp', 4, 'Служба 103', 1),
  ('mosgaz', 5, 'Служба 104', 1),
  ('ogdc', 6, 'ОГДЦ', 1),
  ('codd', 7, 'ЦОДД', 2),
  ('gormost', 8, 'Гормост', 2),
  ('mosgortrans', 9, 'Мосгортранс', 2),
  ('avtomobil-nye-dorogi', 10, 'Автодороги', 2),
  ('mosvodokanal', 11, 'Мосводоканал', 2),
  ('moesk-pao-rosseti-moskovskij-region', 12, 'Россети МР', 3),
  ('moek', 13, 'МОЭК', 3),
  ('dep-zhkh', 14, 'Деп. ЖКХ', 3),
  ('metro', 15, 'Метро', 3),
  ('asu-ns', 16, 'АСУ НС', 3),
  ('departament-rbipk-gku-mosbez', 17, 'Мос.Без.', 4),
  ('112-mos-obl', 18, '112 Мос. обл.', 4),
  ('rsvo', 19, 'ФГУП РСВО', 4),
  ('oek', 20, 'ОЭК', 4),
  ('moslift', 21, 'Мослифт', 4),
  ('mosvodostok', 22, 'Мосводосток', 4),
  ('moskollektor', 23, 'Москоллектор', 4),
  ('voennaya-komendatura', 24, 'Воен. комендатура', 5),
  ('poselenie-chertanovo-severnoe', 25, 'Поселение Чертаново Северное', 5),
  ('poselenie-akademicheskij', 26, 'Поселение Академический', 5),
  ('poselenie-lomonosovskij', 27, 'Поселение Ломоносовский', 5),
  ('poselenie-bibirevo', 28, 'Поселение Бибирево', 6),
  ('poselenie-yuvao', 29, 'Поселение ЮВАО', 6),
  ('poselenie-vao', 30, 'Поселение ВАО', 6),
  ('poselenie-zyablikovo', 31, 'Поселение Зябликово', 7),
  ('poselenie-silino', 32, 'Поселение Силино', 7),
  ('oati', 33, 'ОАТИ', 7),
  ('msppn', 34, 'ГБУ МСППН', 7),
  ('upr-rajona-veshnyaki', 35, 'Упр. района Вешняки', 8),
  ('poselenie-kryukovo', 36, 'Поселение Крюково', 8),
  ('poselenie-matushkino', 37, 'Поселение Матушкино', 8),
  ('poselenie-novogireevo', 38, 'Поселение Новогиреево', 8),
  ('poselenie-savelki', 39, 'Поселение Савелки', 9),
  ('poselenie-staroe-kryukovo', 40, 'Поселение Старое Крюково', 9),
  ('poselenie-chertanovo-yuzhnoe', 41, 'Поселение Чертаново Южное', 9),
  ('poselenie-teplyj-stan', 42, 'Поселение Теплый Стан', 9),
  ('poselenie-ryazanskij', 43, 'Поселение Рязанский', 10),
  ('poselenie-zelao', 44, 'Поселение ЗелАО', 10),
  ('poselenie-pechatniki', 45, 'Поселение Печатники', 10),
  ('poselenie-sao', 46, 'Поселение САО', 11),
  ('poselenie-novokosino', 47, 'Поселение Новокосино', 11),
  ('poselenie-caricyno', 48, 'Поселение Царицыно', 11),
  ('poselenie-mar-ino', 49, 'Поселение Марьино', 11),
  ('poselenie-kuz-minki', 50, 'Поселение Кузьминки', 12),
  ('poselenie-mitino', 51, 'Поселение Митино', 12),
  ('poselenie-savelovskij', 52, 'Поселение Савеловский', 12),
  ('poselenie-schukino', 53, 'Поселение Щукино', 12),
  ('poselenie-pokrovskoe-streshnevo', 54, 'Поселение Покровское-Стрешнево', 13),
  ('poselenie-pervomajskoe', 55, 'Поселение Первомайское', 13),
  ('rzhd-moskovskayazhd', 56, 'МЖД', 13),
  ('112-kal-obl', 57, '112 Кал. обл.', 13),
  ('poselenie-severnoe-tushino', 58, 'Поселение Северное Тушино', 14),
  ('poselenie-nagatinskij-zaton', 59, 'Поселение Нагатинский Затон', 14),
  ('poselenie-dorogomilovo', 60, 'Поселение Дорогомилово', 14),
  ('poselenie-koptevo', 61, 'Поселение Коптево', 14),
  ('poselenie-krylatskoe', 62, 'Поселение Крылатское', 15),
  ('poselenie-tverskoj', 63, 'Поселение Тверской', 15),
  ('poselenie-vnukovo', 64, 'Поселение Внуково', 15),
  ('poselenie-yaroslavskij', 65, 'Поселение Ярославский', 15),
  ('poselenie-yuao', 66, 'Поселение ЮАО', 16),
  ('poselenie-mihajlovo-yarcevskoe', 67, 'Поселение Михайлово-Ярцевское', 16),
  ('poselenie-nekrasovka', 68, 'Поселение Некрасовка', 16),
  ('poselenie-vyhino-zhulebino', 69, 'Поселение Выхино-Жулебино', 17),
  ('poselenie-moskvorech-e-saburovo', 70, 'Поселение Москворечье-Сабурово', 17),
  ('poselenie-yuzao', 71, 'Поселение ЮЗАО', 17),
  ('poselenie-donskoj', 72, 'Поселение Донской', 17),
  ('poselenie-kurkino', 73, 'Поселение Куркино', 18),
  ('poselenie-yuzhnoe-butovo', 74, 'Поселение Южное Бутово', 18),
  ('poselenie-lianozovo', 75, 'Поселение Лианозово', 18),
  ('departament-obrazovaniya', 76, 'Деп. Обр.', 19),
  ('poselenie-lefortovo', 77, 'Поселение Лефортово', 19),
  ('poselenie-zamoskvorech-e', 78, 'Поселение Замоскворечье', 19),
  ('poselenie-svao', 79, 'Поселение СВАО', 19),
  ('poselenie-sokol-niki', 80, 'Поселение Сокольники', 20),
  ('poselenie-izmajlovo', 81, 'Поселение Измайлово', 20),
  ('poselenie-bogorodskoe', 82, 'Поселение Богородское', 20),
  ('poselenie-preobrazhenskij', 83, 'Поселение Преображенский', 20),
  ('poselenie-vostochnoe-izmajlovo', 84, 'Поселение Восточное Измайлово', 21),
  ('poselenie-rostokino', 85, 'Поселение Ростокино', 21),
  ('poselenie-sokolinaya-gora', 86, 'Поселение Соколиная гора', 21),
  ('poselenie-horoshevo-mnevniki', 87, 'Поселение Хорошево-Мневники', 22),
  ('poselenie-gol-yanovo', 88, 'Поселение Гольяново', 22),
  ('poselenie-severnoe-izmajlovo', 89, 'Поселение Северное Измайлово', 22),
  ('poselenie-marfino', 90, 'Поселение Марфино', 22),
  ('poselenie-metrogorodok', 91, 'Поселение Метрогородок', 23),
  ('poselenie-fili-davydkovo', 92, 'Поселение Фили-Давыдково', 23),
  ('poselenie-yuzhnoe-medvedkovo', 93, 'Поселение Южное Медведково', 23),
  ('poselenie-ryazanovskoe', 94, 'Поселение Рязановское', 23),
  ('departament-ppioos', 95, 'Деп. природопользования', 24),
  ('poselenie-timiryazevskij', 96, 'Поселение Тимирязевский', 24),
  ('poselenie-biryulevo-zapadnoe', 97, 'Поселение Бирюлево Западное', 24),
  ('poselenie-ivanovskij', 98, 'Поселение Ивановский', 25),
  ('poselenie-novo-peredelkino', 99, 'Поселение Ново-Переделкино', 25),
  ('poselenie-vostochnoe-degunino', 100, 'Поселение Восточное Дегунино', 25),
  ('poselenie-hovrino', 101, 'Поселение Ховрино', 25),
  ('poselenie-golovinskij', 102, 'Поселение Головинский', 26),
  ('poselenie-beskudnikovo', 103, 'Поселение Бескудниково', 26),
  ('poselenie-levoberezhnyj', 104, 'Поселение Левобережный', 26),
  ('poselenie-dmitrovskij', 105, 'Поселение Дмитровский', 27),
  ('poselenie-sokol', 106, 'Поселение Сокол', 27),
  ('poselenie-molzhaninovskij', 107, 'Поселение Молжаниновский', 27),
  ('poselenie-hamovniki', 108, 'Поселение Хамовники', 27),
  ('poselenie-basmannyj', 109, 'Поселение Басманный', 28),
  ('poselenie-yakimanka', 110, 'Поселение Якиманка', 28),
  ('poselenie-meschanskij', 111, 'Поселение Мещанский', 28),
  ('poselenie-presnenskij', 112, 'Поселение Пресненский', 28),
  ('mvd', 113, 'Служба 102', 29),
  ('poselenie-mosrentgen', 114, 'Поселение Мосрентген', 29),
  ('poselenie-yasenevo', 115, 'Поселение Ясенево', 29),
  ('mgts', 116, 'МГТС', 29),
  ('poselenie-scherbinka', 117, 'Поселение Щербинка', 30),
  ('poselenie-kapotnya', 118, 'Поселение Капотня', 30),
  ('poselenie-troick', 119, 'Поселение Троицк', 30),
  ('poselenie-nagatino-sadovniki', 120, 'Поселение Нагатино-Садовники', 31),
  ('poselenie-klenovskoe', 121, 'Поселение Клёновское', 31),
  ('poselenie-nagornyj', 122, 'Поселение Нагорный', 31),
  ('poselenie-babushkinskij', 123, 'Поселение Бабушкинский', 31),
  ('poselenie-butyrskij', 124, 'Поселение Бутырский', 32),
  ('poselenie-losinoostrovskij', 125, 'Поселение Лосиноостровский', 32),
  ('poselenie-ostankinskij', 126, 'Поселение Останкинский', 32),
  ('poselenie-otradnoe', 127, 'Поселение Отрадное', 32),
  ('poselenie-sviblovo', 128, 'Поселение Свиблово', 33),
  ('poselenie-severnoe-medvedkovo', 129, 'Поселение Северное Медведково', 33),
  ('poselenie-severnyj', 130, 'Поселение Северный', 33),
  ('poselenie-orehovo-borisovo-severnoe', 131, 'Поселение Орехово-Борисово Северное', 34),
  ('poselenie-perovo', 132, 'Поселение Перово', 34),
  ('poselenie-altuf-evskij', 133, 'Поселение Алтуфьевский', 34),
  ('poselenie-mozhajskij', 134, 'Поселение Можайский', 34),
  ('poselenie-ramenki', 135, 'Поселение Раменки', 35),
  ('poselenie-alekseevskij', 136, 'Поселение Алексеевский', 35),
  ('poselenie-kuncevo', 137, 'Поселение Кунцево', 36),
  ('poselenie-troparevo-nikulino', 138, 'Поселение Тропарево-Никулино', 36),
  ('poselenie-vostochnyj', 139, 'Поселение Восточный', 36),
  ('poselenie-aeroport', 140, 'Поселение Аэропорт', 36),
  ('poselenie-zapadnoe-degunino', 141, 'Поселение Западное Дегунино', 37),
  ('poselenie-vojkovskij', 142, 'Поселение Войковский', 37),
  ('poselenie-horoshevskij', 143, 'Поселение Хорошевский', 37),
  ('poselenie-begovoj', 144, 'Поселение Беговой', 38),
  ('poselenie-tekstil-schiki', 145, 'Поселение Текстильщики', 38),
  ('poselenie-moskovskoe', 146, 'Поселение Московское', 38),
  ('poselenie-brateevo', 147, 'Поселение Братеево', 38),
  ('poselenie-filimonkovskoe', 148, 'Поселение Филимонковское', 39),
  ('kanal-imeni-moskvy', 149, 'Канал им. Москвы', 39),
  ('poselenie-lyublino', 150, 'Поселение Люблино', 39),
  ('poselenie-marushkinskoe', 151, 'Поселение Марушкинское', 39),
  ('od-departament-tszn', 152, 'Деп. труда и соц.защиты', 40),
  ('poselenie-orehovo-borisovo-yuzhnoe', 153, 'Поселение Орехово-Борисово Южное', 40),
  ('poselenie-danilovskij', 154, 'Поселение Даниловский', 40),
  ('poselenie-rogovskoe', 155, 'Поселение Роговское', 40),
  ('poselenie-zao', 156, 'Поселение ЗАО', 41),
  ('poselenie-nizhegorodskij', 157, 'Поселение Нижегородский', 41),
  ('poselenie-schapovskoe', 158, 'Поселение Щаповское', 41),
  ('poselenie-yuzhnoportovyj', 159, 'Поселение Южнопортовый', 42),
  ('evazhd', 160, 'ЭВАЖД', 42),
  ('poselenie-cao', 161, 'Поселение ЦАО', 42),
  ('poselenie-cheremushki', 162, 'Поселение Черемушки', 42),
  ('poselenie-mar-ina-roscha', 163, 'Поселение Марьина роща', 43),
  ('poselenie-krasnosel-skij', 164, 'Поселение Красносельский', 43),
  ('poselenie-strogino', 165, 'Поселение Строгино', 43),
  ('poselenie-szao', 166, 'Поселение СЗАО', 44),
  ('poselenie-biryulevo-vostochnoe', 167, 'Поселение Бирюлево Восточное', 44),
  ('poselenie-chertanovo-central-noe', 168, 'Поселение Чертаново Центральное', 44),
  ('poselenie-gagarinskij', 169, 'Поселение Гагаринский', 44),
  ('poselenie-obruchevskij', 170, 'Поселение Обручевский', 45),
  ('poselenie-zyuzino', 171, 'Поселение Зюзино', 45),
  ('poselenie-kon-kovo', 172, 'Поселение Коньково', 45),
  ('poselenie-kotlovka', 173, 'Поселение Котловка', 45),
  ('poselenie-severnoe-butovo', 174, 'Поселение Северное Бутово', 46),
  ('poselenie-kosino-uhtomskij', 175, 'Поселение Косино-Ухтомский', 46),
  ('poselenie-solncevo', 176, 'Поселение Солнцево', 46),
  ('poselenie-taganskij', 177, 'Поселение Таганский', 46),
  ('poselenie-ochakovo-matveevskoe', 178, 'Поселение Очаково-Матвеевское', 47),
  ('poselenie-yuzhnoe-tushino', 179, 'Поселение Южное Тушино', 47),
  ('poselenie-filevskij-park', 180, 'Поселение Филевский парк', 47),
  ('poselenie-tinao', 181, 'Поселение ТиНАО', 48),
  ('poselenie-prospekt-vernadskogo', 182, 'Поселение Проспект Вернадского', 48),
  ('poselenie-desenovskoe', 183, 'Поселение Десеновское', 48),
  ('poselenie-arbat', 184, 'Поселение Арбат', 48),
  ('dezhurno-dispetcherskaya-sluzhba', 185, 'Дежурно-диспетчерская служба', 49),
  ('poselenie-sosenskoe', 186, 'Поселение Сосенское', 49),
  ('poselenie-kokoshkino', 187, 'Поселение Кокошкино', 49),
  ('centrregionvodhoz-moskovsko-okskoe-bvu', 188, 'Центррегионводхоз', 49),
  ('poselenie-voskresenskoe', 189, 'Поселение Воскресенское', 50),
  ('poselenie-krasnopahorskoe', 190, 'Поселение Краснопахорское', 50),
  ('poselenie-voronovskoe', 191, 'Поселение Вороновское', 50),
  ('mosoblgaz', 192, 'Мособлгаз', 50),
  ('poselenie-vnukovskoe', 193, 'Поселение Внуковское', 51),
  ('poselenie-novofedorovskoe', 194, 'Поселение Новофедоровское', 51),
  ('poselenie-kievskij', 195, 'Поселение Киевский', 51),
  ('gbu-ad-vao', 196, 'ГБУ АД ВАО', 51),
  ('gbu-ad-svao', 197, 'ГБУ АД СВАО', 52),
  ('gbu-ad-yuvao', 198, 'ГБУ АД ЮВАО', 52),
  ('gbu-ad-zelao', 199, 'ГБУ АД ЗелАО', 52),
  ('gbu-ad-yuzao', 200, 'ГБУ АД ЮЗАО', 52),
  ('gbu-ad-yuao', 201, 'ГБУ АД ЮАО', 52),
  ('gbu-ad-sao', 202, 'ГБУ АД САО', 53),
  ('gbu-ad-szao', 203, 'ГБУ АД СЗАО', 53),
  ('gbu-ad-cao', 204, 'ГБУ АД ЦАО', 53),
  ('gbu-ad-zao', 205, 'ГБУ АД ЗАО', 53),
  ('departament-kul-tury', 206, 'Департамент культуры города Москвы', 53),
  ('gku-csa-imeni-e-p-glinki', 207, 'ГКУ ЦСА', 54),
  ('komitet-po-turizmu-g-moskvy', 208, 'Мостуризм', 54),
  ('departament-stroitel-stva-goroda-moskvy', 209, 'Департамент строительства', 54),
  ('komitet-veterinarii', 210, 'Комитет ветеринарии', 54),
  ('moszhilinspekciya', 211, 'Мосжилинспекция', 55)
;

-- Типы: все 51 с кадров у нас уже есть. Меняется написание — дословно
-- по выпадающему списку (дефисы, «звонки от работников», «Экологические
-- происшествия») — и порядок: список боевой системы, быстрые плашки —
-- слева направо по кадру пустой карточки.
ALTER TABLE incident_types
    ADD COLUMN listed      boolean NOT NULL DEFAULT true,
    ADD COLUMN sort_order  int,
    ADD COLUMN quick_order int;

UPDATE incident_types SET title = '101', sort_order = 1, quick_order = NULL, frequent = false WHERE id = '101';  -- image2
UPDATE incident_types SET title = '102', sort_order = 2, quick_order = NULL, frequent = false WHERE id = '102';  -- image2
UPDATE incident_types SET title = '103', sort_order = 3, quick_order = NULL, frequent = false WHERE id = '103';  -- image2
UPDATE incident_types SET title = '104', sort_order = 4, quick_order = 3, frequent = true WHERE id = '104';  -- image2
UPDATE incident_types SET title = 'Аварии и происшествия в городском хозяйстве', sort_order = 5, quick_order = NULL, frequent = false WHERE id = 'city-utilities';  -- image2
UPDATE incident_types SET title = 'Аварии и происшествия на транспортных объектах', sort_order = 6, quick_order = NULL, frequent = false WHERE id = 'transport-objects';  -- image3
UPDATE incident_types SET title = 'Аварии на гидротехнических сооружениях', sort_order = 7, quick_order = NULL, frequent = false WHERE id = 'hydro';  -- image3
UPDATE incident_types SET title = 'Аварии на опасных и производственных объектах', sort_order = 8, quick_order = NULL, frequent = false WHERE id = 'hazard-obj';  -- image3
UPDATE incident_types SET title = 'Благодарность службам', sort_order = 9, quick_order = NULL, frequent = false WHERE id = 'gratitude';  -- image3
UPDATE incident_types SET title = 'БПЛА', sort_order = 10, quick_order = NULL, frequent = false WHERE id = 'bpla';  -- image4
UPDATE incident_types SET title = 'Взрыв', sort_order = 11, quick_order = NULL, frequent = false WHERE id = 'explosion';  -- image4
UPDATE incident_types SET title = 'Внутренний звонок (звонки от работников)', sort_order = 12, quick_order = NULL, frequent = false WHERE id = 'internal-call';  -- image4
UPDATE incident_types SET title = 'Вызов на иностранном языке', sort_order = 13, quick_order = 9, frequent = true WHERE id = 'foreign';  -- image4
UPDATE incident_types SET title = 'Дополнительный звонок от заявителя', sort_order = 14, quick_order = NULL, frequent = false WHERE id = 'extra-call';  -- image5
UPDATE incident_types SET title = 'Дорожные помехи', sort_order = 15, quick_order = NULL, frequent = false WHERE id = 'road-hazards';  -- image5
UPDATE incident_types SET title = 'ДТП', sort_order = 16, quick_order = 1, frequent = true WHERE id = 'dtp';  -- image5
UPDATE incident_types SET title = 'Жалоба на действие или бездействие служб', sort_order = 17, quick_order = NULL, frequent = false WHERE id = 'complaint';  -- image5
UPDATE incident_types SET title = 'Животные', sort_order = 18, quick_order = NULL, frequent = false WHERE id = 'animals';  -- image6
UPDATE incident_types SET title = 'Консультация', sort_order = 19, quick_order = 8, frequent = true WHERE id = 'consult';  -- image6
UPDATE incident_types SET title = 'Нецелевой вызов', sort_order = 20, quick_order = NULL, frequent = false WHERE id = 'non-target';  -- image6
UPDATE incident_types SET title = 'Обрушение', sort_order = 21, quick_order = NULL, frequent = false WHERE id = 'collapse';  -- image6
UPDATE incident_types SET title = 'Отзыв о работе 112 Москва', sort_order = 22, quick_order = NULL, frequent = false WHERE id = 'feedback-112';  -- image6
UPDATE incident_types SET title = 'Отмена вызова', sort_order = 23, quick_order = 5, frequent = true WHERE id = 'cancel';  -- image7
UPDATE incident_types SET title = 'Ошибочно набран номер', sort_order = 24, quick_order = 2, frequent = true WHERE id = 'wrong-number';  -- image7
UPDATE incident_types SET title = 'Передача дежурства', sort_order = 25, quick_order = 7, frequent = true WHERE id = 'shift-handover';  -- image7
UPDATE incident_types SET title = 'Помощь службам', sort_order = 26, quick_order = NULL, frequent = false WHERE id = 'help-services';  -- image7
UPDATE incident_types SET title = 'Природная стихия', sort_order = 27, quick_order = NULL, frequent = false WHERE id = 'natural-disaster';  -- image8
UPDATE incident_types SET title = 'Прочие происшествия', sort_order = 28, quick_order = NULL, frequent = false WHERE id = 'other';  -- image8
UPDATE incident_types SET title = 'Радиация', sort_order = 29, quick_order = NULL, frequent = false WHERE id = 'radiation';  -- image8
UPDATE incident_types SET title = 'Разбитый градусник', sort_order = 30, quick_order = NULL, frequent = false WHERE id = 'broken-thermometer';  -- image8
UPDATE incident_types SET title = 'Ребенок в опасности', sort_order = 31, quick_order = NULL, frequent = false WHERE id = 'child-danger';  -- image9
UPDATE incident_types SET title = 'Сбор', sort_order = 32, quick_order = NULL, frequent = false WHERE id = 'gathering';  -- image9
UPDATE incident_types SET title = 'Скопление воды', sort_order = 33, quick_order = NULL, frequent = false WHERE id = 'water-pooling';  -- image9
UPDATE incident_types SET title = 'Смертельный исход', sort_order = 34, quick_order = NULL, frequent = false WHERE id = 'death';  -- image9
UPDATE incident_types SET title = 'Социальная помощь', sort_order = 35, quick_order = NULL, frequent = false WHERE id = 'social-help';  -- image10
UPDATE incident_types SET title = 'Справка-101', sort_order = 36, quick_order = 10, frequent = true WHERE id = 'ref-101';  -- image10
UPDATE incident_types SET title = 'Справка-102', sort_order = 37, quick_order = NULL, frequent = false WHERE id = 'ref-102';  -- image10
UPDATE incident_types SET title = 'Справка-103', sort_order = 38, quick_order = NULL, frequent = false WHERE id = 'ref-103';  -- image10
UPDATE incident_types SET title = 'Справка-104', sort_order = 39, quick_order = NULL, frequent = false WHERE id = 'ref-104';  -- image11
UPDATE incident_types SET title = 'Справка-ГИБДД', sort_order = 40, quick_order = NULL, frequent = false WHERE id = 'ref-gibdd';  -- image11
UPDATE incident_types SET title = 'Справка Городское Хозяйство', sort_order = 41, quick_order = NULL, frequent = false WHERE id = 'ref-city';  -- image11
UPDATE incident_types SET title = 'Справка-МЧС', sort_order = 42, quick_order = NULL, frequent = false WHERE id = 'ref-mchs';  -- image11
UPDATE incident_types SET title = 'Тестовый вызов', sort_order = 43, quick_order = 6, frequent = true WHERE id = 'test-call';  -- image12
UPDATE incident_types SET title = 'Технический сбой (сбой в работе оборудования 112 Москва)', sort_order = 44, quick_order = NULL, frequent = false WHERE id = 'tech-failure';  -- image12
UPDATE incident_types SET title = 'Тренировка', sort_order = 45, quick_order = NULL, frequent = false WHERE id = 'training';  -- image12
UPDATE incident_types SET title = 'Уведомление о ЧС', sort_order = 46, quick_order = NULL, frequent = false WHERE id = 'emergency-notice';  -- image12
UPDATE incident_types SET title = 'Угроза взрыва/террористического акта', sort_order = 47, quick_order = NULL, frequent = false WHERE id = 'blast-threat';  -- image12
UPDATE incident_types SET title = 'Угроза выброса опасных веществ и радиации', sort_order = 48, quick_order = NULL, frequent = false WHERE id = 'hazmat-threat';  -- image13
UPDATE incident_types SET title = 'Угроза обрушения', sort_order = 49, quick_order = NULL, frequent = false WHERE id = 'collapse-threat';  -- image13
UPDATE incident_types SET title = 'Человек в опасности', sort_order = 50, quick_order = 4, frequent = true WHERE id = 'person-danger';  -- image13
UPDATE incident_types SET title = 'Экологические происшествия', sort_order = 51, quick_order = NULL, frequent = false WHERE id = 'ecology';  -- image13

-- Типы, которых нет в выпадающем списке боевой системы, скрываем
UPDATE incident_types SET listed = false, frequent = false, quick_order = NULL
 WHERE sort_order IS NULL;
