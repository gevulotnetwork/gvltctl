test: qemu

disk:
	RUST_LOG=trace cargo r -- build --force --rootfs-dir ./rootfs

disk-container:
	RUST_LOG=trace cargo r -- build --force --container localhost/my-container-image:latest

disk-containerfile:
	RUST_LOG=trace cargo r -- build --force --containerfile ./Containerfile

disk-nvidia:
	RUST_LOG=trace cargo r -- build --force --containerfile ./Containerfile --nvidia-drivers

test-disk:
	RUST_LOG=trace cargo r -- build --force --containerfile ./Containerfile --no-gevulot-runtime

base:
	RUST_LOG=trace cargo r -- build --generate-base-image --output base.img

qemu: test-disk
	qemu-system-x86_64 -machine q35 -enable-kvm -nographic --hda disk.img

clean:
	rm -f disk.img

.PHONY: disk disk-container clean qemu test base
