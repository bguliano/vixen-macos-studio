use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    sync::Arc,
};
use vixen_session::{app::AppCore, sequence::SequenceSession};

struct Connection {
    stream: UnixStream,
    reader: BufReader<UnixStream>,
    next: u64,
}
impl Connection {
    fn open(root: &std::path::Path) -> Self {
        let stream = studio_mcp::socket::connect(root).unwrap().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .unwrap();
        let reader = BufReader::new(stream.try_clone().unwrap());
        let mut connection = Self {
            stream,
            reader,
            next: 1,
        };
        connection.rpc(
            "initialize",
            json!({"protocolVersion":"2024-11-05","clientInfo":{},"capabilities":{}}),
        );
        connection
    }
    fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        writeln!(
            self.stream,
            "{}",
            json!({"jsonrpc":"2.0","id":self.next,"method":method,"params":params})
        )
        .unwrap();
        let mut line = String::new();
        self.reader.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap()
    }
    fn tool(&mut self, name: &str, arguments: Value) -> Value {
        tool_result(self.rpc("tools/call", json!({"name":name,"arguments":arguments})))
    }
}
fn tool_result(reply: Value) -> Value {
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(reply["result"]["isError"], false, "{text}");
    serde_json::from_str(text).unwrap()
}

/// Issue #9: every connection to the app gets the document the app has open, under one ID, and a headless bridge says
/// it is not shared with the app.
#[test]
fn sequence_ids_are_stable_across_connections_and_reopens() {
    // The app reaches the show through a symlinked folder, so its spelling of the file differs from the canonical path
    // the MCP side builds (as /var/folders vs /private/var/folders does on macOS).
    let base =
        std::env::temp_dir().join(format!("studio-sequence-identity-{}", std::process::id()));
    std::fs::create_dir_all(base.join("show")).unwrap();
    std::os::unix::fs::symlink(base.join("show"), base.join("alias")).unwrap();
    let root = base.join("alias");
    {
        // Write the file from a core that is gone again, as a show the app is about to open.
        let writer = AppCore::open(root.clone(), true).unwrap();
        let created = vixen_session::studio_sequence::execute(
            &writer,
            "sequence_create",
            &json!({"lengthMs":10000}),
            false,
        )
        .unwrap();
        vixen_session::studio_sequence::execute(
            &writer,
            "sequence_save",
            &json!({"sequenceId":created["sequenceId"],"file":"Song.tim","confirm":true}),
            false,
        )
        .unwrap();
        writer.studio_display.stop();
    }
    let core = Arc::new(AppCore::open(root.clone(), true).unwrap());

    // The app opens the file the way a window does.
    let app_open = |core: &AppCore| {
        let session = SequenceSession::open(
            &root.join("Sequence").join("Song.tim"),
            &core.session_environment(),
        )
        .unwrap();
        // parking_lot's Mutex, through From, without a dependency of this crate's own.
        let shared = Arc::new(session.into());
        core.register_sequence_session(&shared);
        shared
    };
    let window = app_open(&core);
    let server = studio_mcp::socket::SocketServer::start(&core).unwrap();

    let mut first = Connection::open(&root);
    let mut second = Connection::open(&root);
    let a = first.tool("sequence_open", json!({"file":"Song.tim"}));
    let b = second.tool("sequence_open", json!({"file":"Song.tim"}));
    assert_eq!(a["alreadyOpen"], true, "{a}");
    assert_eq!(b["alreadyOpen"], true, "{b}");
    assert_eq!(a["sequenceId"], b["sequenceId"]);
    assert_eq!(a["sessionId"], window.lock().playback_id().to_string());
    assert_eq!(a["sharedWithApp"], true);
    assert_eq!(a["file"], "Song.tim");
    assert_eq!(a["modified"], false);
    assert!(a["path"].as_str().unwrap().ends_with("Sequence/Song.tim"));
    assert_eq!(
        core.open_sequence_sessions().len(),
        1,
        "no second copy of the document"
    );

    // An ID from one connection works on the other, and edits land in the app's document.
    let id = a["sequenceId"].clone();
    second.tool("sequence_length", json!({"sequenceId":id,"lengthMs":20000}));
    assert_eq!(window.lock().length().total_ms(), 20000.0);
    let status = first.tool("sequence_status", json!({"sequenceId":id}));
    assert_eq!(status["modified"], true, "{status}");
    assert_eq!(status["file"], "Song.tim");
    assert_eq!(status["sharedWithApp"], true);

    // The app closes and reopens the file: the same ID addresses the new session.
    drop(window);
    let window = app_open(&core);
    let reopened = first.tool("sequence_open", json!({"file":"Song.tim"}));
    assert_eq!(reopened["sequenceId"], id);
    assert_eq!(
        reopened["sessionId"],
        window.lock().playback_id().to_string()
    );
    assert_eq!(
        second.tool("sequence_status", json!({"sequenceId":id}))["lengthMs"],
        10000.0
    );

    // A headless bridge has its own core: same file ID, but it says so.
    let mut headless = studio_mcp::Server::open(&root).unwrap();
    headless.dispatch(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","clientInfo":{},"capabilities":{}}}));
    let call = |server: &mut studio_mcp::Server, name: &str, arguments: Value| {
        tool_result(server.dispatch(json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":name,"arguments":arguments}})).unwrap())
    };
    let copy = call(&mut headless, "sequence_open", json!({"file":"Song.tim"}));
    assert_eq!(copy["sequenceId"], id);
    assert_eq!(copy["sharedWithApp"], false);
    let status = call(&mut headless, "studio_status", json!({}));
    assert_eq!(status["mode"], "headlessSnapshot", "{status}");
    assert_eq!(status["sharedWithApp"], false);
    assert_eq!(
        first.tool("studio_status", json!({}))["mode"],
        "liveSession"
    );

    drop(headless);
    drop((first, second, window));
    core.studio_display.stop();
    drop(server);
    drop(core);
    std::fs::remove_dir_all(base).unwrap();
}
