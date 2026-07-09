APP := varmint
BIN := target/release/$(APP)
ENTITLEMENTS := entitlements.xml

BACKEND ?= moltenvk

VIRGL_UTM := $(HOME)/dev/varmint-deps/virglrenderer-utm
EPOXY_UTM := $(HOME)/dev/varmint-deps/libepoxy-utm
ANGLE_UTM := $(HOME)/dev/varmint-deps/angle-utm
MVK_UTM := /opt/homebrew/opt/molten-vk-utm

KK_UTM := $(HOME)/dev/varmint-deps/kosmickrisp-utm
KK_ICD := $(KK_UTM)/share/vulkan/icd.d/kosmickrisp_mesa_icd.aarch64.json
VULKAN_LOADER := $(shell brew --prefix vulkan-loader)

ifeq ($(BACKEND),moltenvk)
GRAPHICS_PKG_CONFIG_PATH := $(VIRGL_UTM)/lib/pkgconfig:$(EPOXY_UTM)/lib/pkgconfig:$(ANGLE_UTM)/lib/pkgconfig:$(MVK_UTM)/lib/pkgconfig
GRAPHICS_DYLD_PATH := $(VIRGL_UTM)/lib:$(EPOXY_UTM)/lib:$(ANGLE_UTM)/lib:$(MVK_UTM)/lib
GRAPHICS_RUSTFLAGS := -L $(VIRGL_UTM)/lib -L $(EPOXY_UTM)/lib -L $(ANGLE_UTM)/lib -L $(MVK_UTM)/lib -L /opt/homebrew/lib
GRAPHICS_ENV :=
else ifeq ($(BACKEND),kosmickrisp)
GRAPHICS_PKG_CONFIG_PATH := $(VIRGL_UTM)/lib/pkgconfig:$(EPOXY_UTM)/lib/pkgconfig:$(ANGLE_UTM)/lib/pkgconfig:$(VULKAN_LOADER)/lib/pkgconfig
GRAPHICS_DYLD_PATH := $(VIRGL_UTM)/lib:$(EPOXY_UTM)/lib:$(ANGLE_UTM)/lib:$(VULKAN_LOADER)/lib:$(KK_UTM)/lib
GRAPHICS_RUSTFLAGS := -L $(VIRGL_UTM)/lib -L $(EPOXY_UTM)/lib -L $(ANGLE_UTM)/lib -L $(VULKAN_LOADER)/lib -L $(KK_UTM)/lib -L /opt/homebrew/lib
GRAPHICS_ENV := VK_ICD_FILENAMES="$(KK_ICD)"
else
$(error Unknown BACKEND '$(BACKEND)'. Use BACKEND=moltenvk or BACKEND=kosmickrisp)
endif

.PHONY: release dtb run check-dylibs clean

release:
	PKG_CONFIG_PATH="$(GRAPHICS_PKG_CONFIG_PATH):$$PKG_CONFIG_PATH" \
	RUSTFLAGS="$(GRAPHICS_RUSTFLAGS)" \
	cargo build --release
	codesign --sign - --entitlements $(ENTITLEMENTS) --deep --force $(BIN)

dtb:
	dtc -I dts -O dtb -o ./artifacts/guest.dtb ./dts/guest.dts

run: dtb release
	@echo "BACKEND=$(BACKEND)"
	VARMINT_FENCE_POLL_US=1000 \
	$(GRAPHICS_ENV) \
	DYLD_FRAMEWORK_PATH="$(ANGLE_UTM)/lib:$$DYLD_FRAMEWORK_PATH" \
	DYLD_LIBRARY_PATH="$(GRAPHICS_DYLD_PATH):$$DYLD_LIBRARY_PATH" \
	$(BIN) 2> vmm.log

check-dylibs:
	@echo "BACKEND=$(BACKEND)"
	otool -L $(BIN) | egrep -i 'virgl|epoxy|MoltenVK|EGL|GLES|vulkan|kosmic|mesa' || true
	@echo
	@echo "virglrenderer:"
	@otool -L "$(VIRGL_UTM)/lib/libvirglrenderer.1.dylib" | egrep -i 'virgl|epoxy|MoltenVK|EGL|GLES|vulkan|kosmic|mesa|System' || true

clean:
	cargo clean
