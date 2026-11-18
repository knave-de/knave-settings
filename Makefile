PREFIX ?= /usr/local
APP_ID := de.knave.Settings

.PHONY: build install uninstall
build:
	cargo build --release --locked

install: build
	install -Dm755 target/release/knave-settings "$(DESTDIR)$(PREFIX)/bin/knave-settings"
	install -Dm644 "data/$(APP_ID).desktop" "$(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop"
	install -Dm644 data/icons/knave-settings.svg "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg"

uninstall:
	rm -f "$(DESTDIR)$(PREFIX)/bin/knave-settings"
	rm -f "$(DESTDIR)$(PREFIX)/share/applications/$(APP_ID).desktop"
	rm -f "$(DESTDIR)$(PREFIX)/share/icons/hicolor/scalable/apps/$(APP_ID).svg"
