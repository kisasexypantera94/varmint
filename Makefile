APP_BUNDLE := $(CURDIR)/dist/Varmint.app
APP_BIN := $(APP_BUNDLE)/Contents/MacOS/varmint
GUEST_DIR := $(CURDIR)/build/guest

KERNEL ?= $(GUEST_DIR)/Image
INITRD ?= $(GUEST_DIR)/initrd
BASE_IMAGE ?= $(GUEST_DIR)/varmint-debian.raw.zst
CONFIG ?= $(CURDIR)/gaming.varmint

.PHONY: app bundle dependencies guest-image run clean

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

.PHONY: neptune-sync neptune-build neptune-install

neptune-sync:
	./scripts/neptune-sync.sh --force

neptune-build:
	./scripts/neptune-build.sh

neptune-install:
	@test -n "$(APPID)" || (echo "usage: make neptune-install APPID=<steam-appid> [EXE='path/to/game.exe']" >&2; exit 2)
	./scripts/neptune-install.sh "$(APPID)" "$(EXE)"

.PHONY: neptune-capture

neptune-capture:
	./scripts/neptune-capture.sh


