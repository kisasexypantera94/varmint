APP_BUNDLE := $(CURDIR)/dist/Varmint.app
APP_BIN := $(APP_BUNDLE)/Contents/MacOS/varmint
GUEST_DIR := $(CURDIR)/build/guest
PREFIX := $(CURDIR)/build/prefix

KERNEL ?= $(GUEST_DIR)/Image
INITRD ?= $(GUEST_DIR)/initrd
BASE_IMAGE ?= $(GUEST_DIR)/varmint-debian.raw.zst
CONFIG ?= $(CURDIR)/gaming.varmint

.PHONY: app bundle dependencies guest-image run clean test

app: guest-image
	./scripts/build-app.sh --kernel "$(KERNEL)" --initrd "$(INITRD)" --base-image "$(BASE_IMAGE)"

bundle: guest-image
	./scripts/build-app.sh --skip-dependencies --kernel "$(KERNEL)" --initrd "$(INITRD)" --base-image "$(BASE_IMAGE)"

dependencies:
	./scripts/build-app.sh --dependencies-only

guest-image:
	./guest/build-image.sh

run: bundle
	VARMINT_FENCE_POLL_US=1000 \
	"$(APP_BIN)" "$(CONFIG)" 2> vmm.log

clean:
	rm -rf "$(CURDIR)/build" "$(CURDIR)/dist"

test:
	@test -f "$(PREFIX)/lib/libvirglrenderer.1.dylib" || 		(echo "missing build dependencies; run: make dependencies" >&2; exit 2)
	PKG_CONFIG_PATH="$(PREFIX)/lib/pkgconfig:$${PKG_CONFIG_PATH:-}" 	RUSTFLAGS="-L native=$(PREFIX)/lib $${RUSTFLAGS:-}" 	DYLD_LIBRARY_PATH="$(PREFIX)/lib:$${DYLD_LIBRARY_PATH:-}" 	cargo test

.PHONY: neptune-sync neptune-sync-force neptune-build game-graphics

neptune-sync:
	./scripts/neptune-sync.sh

neptune-sync-force:
	./scripts/neptune-sync.sh --force

neptune-build:
	./scripts/neptune-build.sh

game-graphics:
	@test -n "$(APPID)" || (echo "usage: make game-graphics APPID=<steam-appid> MODE=<status|venus|neptune|neptune-dx12> [EXE='path/to/game.exe']" >&2; exit 2)
	./scripts/game-graphics.sh "$(APPID)" "$(or $(MODE),status)" "$(EXE)"

.PHONY: neptune-capture

neptune-capture:
	./scripts/neptune-capture.sh


