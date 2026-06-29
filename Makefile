APP := varmint
BIN := target/release/$(APP)
ENTITLEMENTS := entitlements.xml

VIRGL_UTM := $(HOME)/dev/varmint-deps/virglrenderer-utm
EPOXY_UTM := $(HOME)/dev/varmint-deps/libepoxy-utm
ANGLE_UTM := $(HOME)/dev/varmint-deps/angle-utm
MVK_UTM := /opt/homebrew/opt/molten-vk-utm

GRAPHICS_PKG_CONFIG_PATH := $(VIRGL_UTM)/lib/pkgconfig:$(EPOXY_UTM)/lib/pkgconfig:$(ANGLE_UTM)/lib/pkgconfig:$(MVK_UTM)/lib/pkgconfig
GRAPHICS_DYLD_PATH := $(VIRGL_UTM)/lib:$(EPOXY_UTM)/lib:$(ANGLE_UTM)/lib:$(MVK_UTM)/lib
GRAPHICS_RUSTFLAGS := -L $(VIRGL_UTM)/lib -L $(EPOXY_UTM)/lib -L $(ANGLE_UTM)/lib -L $(MVK_UTM)/lib -L /opt/homebrew/lib

.PHONY: release dtb run check-dylibs clean

release:
	PKG_CONFIG_PATH="$(GRAPHICS_PKG_CONFIG_PATH):$$PKG_CONFIG_PATH" \
	RUSTFLAGS="$(GRAPHICS_RUSTFLAGS)" \
	cargo build --release
	codesign --sign - --entitlements $(ENTITLEMENTS) --deep --force $(BIN)

dtb:
	dtc -I dts -O dtb -o ./artifacts/guest.dtb ./dts/guest.dts

run: dtb release
	VARMINT_FENCE_POLL_US=1000 \
	DYLD_FRAMEWORK_PATH="$(ANGLE_UTM)/lib:$$DYLD_FRAMEWORK_PATH" \
	DYLD_LIBRARY_PATH="$(GRAPHICS_DYLD_PATH):$$DYLD_LIBRARY_PATH" \
	$(BIN) 2> vmm.log

check-dylibs:
	otool -L $(BIN) | egrep -i 'virgl|epoxy|MoltenVK|EGL|GLES' || true

clean:
	cargo clean
