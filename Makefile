APP := varmint
BIN := target/release/$(APP)
ENTITLEMENTS := entitlements.xml

VIRGL_UTM := $(shell brew --prefix local/varmint/virglrenderer-utm)
MVK_UTM := $(shell brew --prefix local/varmint/molten-vk-utm)

.PHONY: release run clean

release:
	RUSTFLAGS="-L $(VIRGL_UTM)/lib -L $(MVK_UTM)/lib -L /opt/homebrew/lib" cargo build --release
	codesign --sign - --entitlements $(ENTITLEMENTS) --deep --force $(BIN)

dtb:
	dtc -I dts -O dtb -o ./artifacts/guest.dtb ./dts/guest.dts

run: dtb release
	$(BIN) 2> vmm.log

clean:
	cargo clean