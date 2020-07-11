INSTALL_DIR ?= /usr/local/bin

all: build

build:
	go build cmd/tarfs.go

clean:
	go clean
	rm tarfs

install: build
	install tarfs ${INSTALL_DIR}

.PHONY: all build clean install
