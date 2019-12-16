# Go parameters
GOPATH=/home/richard/Code/tarfs
GOCMD=go
GOBUILD=$(GOCMD) build
GOCLEAN=$(GOCMD) clean
GOTEST=$(GOCMD) test
GOGET=$(GOCMD) get
BINARY_NAME=tarfs
SRC_PATH=./src
BIN_PATH=./bin

.RECIPEPREFIX+=

export GOPATH

all: test build

build:
    $(GOBUILD) -o $(BIN_PATH)/$(BINARY_NAME) -v $(SRC_PATH)/$(BINARY_NAME)

test:
    $(GOTEST) -v $(SRC_PATH)/$(BINARY_NAME)

clean:
    $(GOCLEAN)
    rm -f $(BIN_PATH)/$(BINARY_NAME)

run: build
   $(BIN_PATH)/$(BINARY_NAME)

deps:
    $(GOGET) bazil.org/fuse
    $(GOGET) golang.org/x/net/context

vim:
    vim
