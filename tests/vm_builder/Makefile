test: qemu

disk:
	RUST_LOG=trace cargo r -- build --force --rootfs-dir ./rootfs

base:
	RUST_LOG=trace cargo r -- build --generate-base-image --output base.img

qemu: disk
	qemu-system-x86_64 -machine q35 -enable-kvm -nographic --hda disk.img

clean:
	rm -f disk.img

.PHONY: disk clean qemu test base
