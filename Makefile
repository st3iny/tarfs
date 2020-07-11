INSTALL_DIR = /usr/local/bin

all: build

build:
	go build cmd/tarfs.go

clean:
	go clean
	rm tarfs

install: build
	sudo install tarfs ${INSTALL_DIR}

.PHONY: build clean install
