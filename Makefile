APP_BUNDLE := $(CURDIR)/dist/Varmint.app
APP_BIN := $(APP_BUNDLE)/Contents/MacOS/varmint

KERNEL ?=
INITRD ?=
CONFIG ?= $(CURDIR)/gaming.varmint

.PHONY: app bundle dependencies run run-sudo clean

app:
	@test -n "$(KERNEL)" || { echo "error: KERNEL is required" >&2; exit 1; }
	@test -n "$(INITRD)" || { echo "error: INITRD is required" >&2; exit 1; }
	./scripts/build-app.sh --kernel "$(KERNEL)" --initrd "$(INITRD)"

bundle:
	@test -n "$(KERNEL)" || { echo "error: KERNEL is required" >&2; exit 1; }
	@test -n "$(INITRD)" || { echo "error: INITRD is required" >&2; exit 1; }
	./scripts/build-app.sh --skip-dependencies --kernel "$(KERNEL)" --initrd "$(INITRD)"

dependencies:
	./scripts/build-app.sh --dependencies-only

run: bundle
	VARMINT_FENCE_POLL_US=1000 \
	"$(APP_BIN)" "$(CONFIG)" 2> vmm.log

run-sudo: bundle
	sudo env \
		VARMINT_FENCE_POLL_US=1000 \
		"$(APP_BIN)" "$(CONFIG)" 2> vmm.log

clean:
	rm -rf "$(CURDIR)/build" "$(CURDIR)/dist"
