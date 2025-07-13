LINUX_NAME := firecracker-vm

build:
	@cargo build

test:
	@cargo nextest run --all-features

release:
	@cargo release tag --execute
	@git cliff -o CHANGELOG.md
	@git commit -a -n -m "Update CHANGELOG.md" || true
	@git push origin master
	@cargo release push --execute

update-submodule:
	@git submodule update --init --recursive --remote

run-linux:
	@limactl start --name $(LINUX_NAME) ./linux.yaml

stop-linux:
	@limactl stop $(LINUX_NAME)

shell-linux:
	@limactl shell $(LINUX_NAME)

delete-linux:
	@limactl delete $(LINUX_NAME)

.PHONY: build test release update-submodule run-linux stop-linux shell-linux delete-linux
