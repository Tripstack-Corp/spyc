//! A jar that says it has an owner but names no pid for it: what a jar says
//! for a moment as an owner it took leaves. A retake asks the jar again, and
//! `^a R`'s check of who owns the jar names no other spyc on it (the flake in
//! `a_tab_whose_jar_is_silent_past_one_welcome_is_taken_back_once_it_answers`).

// A module of its own, which the jar's source scans read as a test's.
#[cfg(test)]
mod checks {
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use spyc_jar::proto::{self, Msg, Role};

    use super::super::place::{HOLD, Place};
    use super::super::test_dir::TestDir;

    /// How long the fake jar waits for a client's next frame.
    const WAIT: Duration = Duration::from_secs(5);

    /// A place in `dir` whose jars this test never starts.
    fn place_in(dir: &Path) -> Place {
        Place {
            binary: PathBuf::from("/nonexistent"),
            dirs: vec![dir.join("spyc"), dir.join("spyc").join("jars")],
            tokens: dir.join("tokens"),
            hold: HOLD,
            repaint_hold: Duration::from_secs(2),
        }
    }

    /// A jar with an owner it can't name, which is what a jar says for a moment
    /// as an owner it took leaves (its connection gone from its table, its owner
    /// not yet cleared). It refuses its first owner's hello as taken, describes
    /// that owner to a lookup as connected with no pid, and refuses each later
    /// owner's hello for another reason, so what a client did next shows. It
    /// counts the owners' hellos.
    struct UnnamedOwnerJar {
        socket: PathBuf,
        hellos: std::sync::Arc<std::sync::atomic::AtomicUsize>,
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl UnnamedOwnerJar {
        fn at(socket: &Path) -> Self {
            use std::sync::atomic::Ordering::SeqCst;
            let listener = UnixListener::bind(socket).unwrap();
            let hellos = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let (counted, stopped) = (hellos.clone(), stop.clone());
            let thread = std::thread::spawn(move || {
                for conn in listener.incoming() {
                    if stopped.load(SeqCst) {
                        break;
                    }
                    let Ok(mut conn) = conn else { continue };
                    let _ = conn.set_read_timeout(Some(WAIT));
                    // A connection that only asks whether the jar answers sends
                    // nothing.
                    let Ok(Some(Msg::Hello { role, .. })) = proto::read_msg(&mut conn) else {
                        continue;
                    };
                    let reply = if role == Role::Owner {
                        if counted.fetch_add(1, SeqCst) == 0 {
                            Msg::Refused {
                                reason: proto::reason::OWNER_PRESENT,
                                why: "the jar already has an owner".into(),
                            }
                        } else {
                            Msg::Refused {
                                reason: proto::reason::ROLE_NOT_PERMITTED,
                                why: "asked again".into(),
                            }
                        }
                    } else {
                        let welcome = Msg::Welcome {
                            version: proto::VERSION,
                            child_pid: 0,
                            head: 0,
                            base: 0,
                            rows: 24,
                            cols: 80,
                        };
                        if proto::write_msg(&mut conn, &welcome).is_err()
                            || !matches!(proto::read_msg(&mut conn), Ok(Some(Msg::Describe)))
                        {
                            continue;
                        }
                        Msg::Description {
                            fields: vec![
                                ("owner".into(), "connected".into()),
                                ("owner_pid".into(), String::new()),
                            ],
                        }
                    };
                    let _ = proto::write_msg(&mut conn, &reply);
                }
            });
            Self {
                socket: socket.to_path_buf(),
                hellos,
                stop,
                thread: Some(thread),
            }
        }

        fn owner_hellos(&self) -> usize {
            self.hellos.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl Drop for UnnamedOwnerJar {
        fn drop(&mut self) {
            self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
            let _ = UnixStream::connect(&self.socket);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    /// A retake whose jar says it has an owner it can't name asks the jar again:
    /// a pid the jar didn't give names no other spyc. (The flake in
    /// `a_tab_whose_jar_is_silent_past_one_welcome_is_taken_back_once_it_answers`:
    /// after the jar is continued, the hello the retake's first try left behind
    /// can be taken as its owner and let go, and a description between the two
    /// names no pid.)
    #[test]
    fn a_retake_asks_again_when_its_jar_names_no_owner() {
        let dir = TestDir::new("jo");
        let place = place_in(dir.path());
        place.prepare().unwrap();
        let socket = place.socket("dddd");
        super::super::tokens::save(&place, &socket, b"t").unwrap();
        let jar = UnnamedOwnerJar::at(&socket);
        let spec = super::super::Spec {
            place,
            name: "dddd".into(),
            adopt: Some(socket.clone()),
            retake: true,
            exit_linger: None,
            watch: None,
        };
        let ended = super::super::take::back(&spec, &socket, (24, 80)).unwrap();
        assert!(ended.is_some(), "premise: the jar runs");
        assert_eq!(
            jar.owner_hellos(),
            2,
            "the retake took an owner the jar can't name for another spyc's, and stopped asking"
        );
    }

    /// `^a R` asks the jar who owns it: an owner the jar can't name is no other
    /// spyc's, so the restart isn't refused on it.
    #[test]
    fn a_jar_that_names_no_owner_is_not_another_spycs() {
        let dir = TestDir::new("jo");
        let place = place_in(dir.path());
        place.prepare().unwrap();
        let socket = place.socket("eeee");
        let _jar = UnnamedOwnerJar::at(&socket);
        assert_eq!(
            super::super::take::owned_elsewhere_now(&socket, WAIT),
            None,
            "an owner the jar can't name was taken for another spyc's"
        );
    }
}
