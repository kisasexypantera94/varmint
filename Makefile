APP := varmint
BIN := target/release/$(APP)
ENTITLEMENTS := entitlements.xml

.PHONY: release sign run clean

release:
	cargo build --release
	codesign --sign - --entitlements $(ENTITLEMENTS) --deep --force $(BIN)

dtb:
	dtc -I dts -O dtb -o ./artifacts/guest.dtb ./dts/guest.dts

run: dtb release
	$(BIN) 2> vmm.log

clean:
	cargo clean