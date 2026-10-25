use perpetua_formats::{
    evt::{EventPriority, EvtFormat, EvtVersion, ScheduledEvent},
    ini_formats::{JinglePalette, M3uFormat, RotationPlaylist},
    lst::LstFormat,
    sweeper_db::{SweeperDatabase, SweeperRecord},
    TrackItem, TrackKind,
};

#[test]
fn test_lst_roundtrip_and_virtual_commands() {
    let raw_lst = "\
5
215000\tC:\\Musica\\Cancion.mp3~4
0\t.time
0\t.temperature
45000\t45.satellite
60000\thttp://stream.local:8000/live*60.stream
";
    let items = LstFormat::parse_str(raw_lst).expect("parse .lst");
    assert_eq!(items.len(), 5);
    assert_eq!(items[0].custom_overlap_sec, Some(4));
    assert_eq!(
        items[0].kind,
        TrackKind::AudioFile {
            path: "C:\\Musica\\Cancion.mp3".to_string()
        }
    );
    assert_eq!(items[1].kind, TrackKind::TimeAnnouncement);
    assert_eq!(items[2].kind, TrackKind::TemperatureAnnouncement);
    assert_eq!(items[3].kind, TrackKind::SatelliteConnection { seconds: 45 });
    assert_eq!(
        items[4].kind,
        TrackKind::InternetStream {
            url: "http://stream.local:8000/live".to_string(),
            seconds: 60
        }
    );

    let serialized = LstFormat::serialize_to_string(&items);
    assert_eq!(serialized, raw_lst);
}

#[test]
fn test_evt03_binary_roundtrip() {
    let events = vec![ScheduledEvent {
        id: "evt-1".to_string(),
        immediate: true,
        day_hour_sec: 36000,
        day_mask: [false, true, true, true, true, true, false],
        track: TrackItem::from_raw_spec(0, ".time"),
        start_date: "10/18/26 08:00:00".to_string(),
        expiration_date: "10/18/27 08:00:00".to_string(),
        expires: true,
        priority: EventPriority::High,
        hour_bits: 0x00FF_FFFF,
        enabled: true,
        max_wait_minutes: 5,
    }];

    let bytes = EvtFormat::serialize_bytes(&events).expect("serialize .evt");
    assert_eq!(&bytes[0..5], b"EVT03");

    let (version, decoded) = EvtFormat::parse_bytes(&bytes).expect("parse .evt");
    assert_eq!(version, EvtVersion::V3);
    assert_eq!(decoded.len(), 1);
    assert!(decoded[0].immediate);
    assert_eq!(decoded[0].day_hour_sec, 36000);
    assert_eq!(decoded[0].priority, EventPriority::High);
    assert_eq!(decoded[0].hour_bits, 0x00FF_FFFF);
    assert_eq!(decoded[0].max_wait_minutes, 5);
}

#[test]
fn test_rotation_and_jingles_and_m3u() {
    let rot_ini = "\
[playlist]\r
NumberOfEntries=2\r
File0=C:\\Spots\\spotA.mp3\r
Length0=15000\r
File1=C:\\Spots\\spotB.mp3\r
Length1=20000\r
NextIndex=1\r
";
    let mut rot = RotationPlaylist::parse_ini(rot_ini);
    assert_eq!(rot.entries.len(), 2);
    assert_eq!(rot.next_index, 1);
    let next = rot.next_track().expect("next rotation item");
    assert_eq!(next.to_raw_spec(), "C:\\Spots\\spotB.mp3");
    assert_eq!(rot.next_index, 0);

    let mut palette = JinglePalette::default();
    palette.page_names[0] = "Deportes".to_string();
    palette.slots[0].file_path = "C:\\Jingles\\gol.wav".to_string();
    palette.slots[0].sweeper_ducking = true;
    let zsd = palette.serialize_zsd();
    let parsed_palette = JinglePalette::parse_zsd(&zsd);
    assert_eq!(parsed_palette.page_names[0], "Deportes");
    assert!(parsed_palette.slots[0].sweeper_ducking);

    let m3u = M3uFormat::serialize_to_string(&rot.entries);
    let parsed_m3u = M3uFormat::parse_str(&m3u);
    assert_eq!(parsed_m3u.len(), 2);
}

#[test]
fn test_sweeper_sqlite_database() {
    let db = SweeperDatabase::open_in_memory().expect("open sqlite in memory");
    db.upsert(&SweeperRecord {
        target_song_file: "*".to_string(),
        sweeper_file: "C:\\Pisadores\\general.mp3".to_string(),
        intro_seconds: 7,
        enabled: true,
    })
    .expect("upsert wildcard sweeper");

    let picked = db
        .pick_sweeper_for_track("C:\\Musica\\cualquiera.mp3")
        .expect("query sweeper")
        .expect("found wildcard sweeper");
    assert_eq!(picked.sweeper_file, "C:\\Pisadores\\general.mp3");
    assert_eq!(picked.intro_seconds, 7);
}
