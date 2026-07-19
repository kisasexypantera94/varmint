APP_BUNDLE := $(CURDIR)/dist/Varmint.app
APP_BIN := $(APP_BUNDLE)/Contents/MacOS/varmint

KERNEL ?= $(CURDIR)/artifacts/kernel/Image
INITRD ?= $(CURDIR)/artifacts/kernel/initrd
DISK ?= $(CURDIR)/dev0.img

.PHONY: app bundle dependencies run run-sudo clean

app:
	./scripts/build-app.sh --kernel "$(KERNEL)" --initrd "$(INITRD)"

bundle:
	./scripts/build-app.sh --skip-dependencies --kernel "$(KERNEL)" --initrd "$(INITRD)"

dependencies:
	./scripts/build-app.sh --dependencies-only

run: bundle
	VARMINT_DISK="$(DISK)" \
	VARMINT_FENCE_POLL_US=1000 \
	"$(APP_BIN)" 2> vmm.log

run-sudo: bundle
	sudo env \
		VARMINT_DISK="$(DISK)" \
		VARMINT_FENCE_POLL_US=1000 \
		"$(APP_BIN)" 2> vmm.log

clean:
	rm -rf "$(CURDIR)/build" "$(CURDIR)/dist"
