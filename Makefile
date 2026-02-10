SHELL := /bin/bash
PROJECT := allure3-docker-service
IMAGE := $(PROJECT):dev
PORT := 8080
DATA_DIR := $(CURDIR)/data

# Можно переопределить при вызове:
# make docker-build IMAGE=allure-wrapper:1.0.0
# make run PORT=9090
# make docker-run DATA_DIR=/tmp/allure-data
RUST_LOG ?= info

.DEFAULT_GOAL := help

.PHONY: help
help:
	@echo "Targets:"
	@echo "  build              - cargo build (debug)"
	@echo "  build-release      - cargo build --release"
	@echo "  run                - run locally (DATA_DIR=$(DATA_DIR), PORT=$(PORT))"
	@echo "  test               - cargo test"
	@echo "  fmt                - cargo fmt"
	@echo "  clippy             - cargo clippy (deny warnings)"
	@echo "  clean              - cargo clean"
	@echo "  docker-build       - build docker image ($(IMAGE))"
	@echo "  docker-run         - run docker container (port $(PORT), volume $(DATA_DIR))"
	@echo "  docker-stop        - stop docker container named $(PROJECT)"
	@echo "  docker-logs        - follow logs for container named $(PROJECT)"
	@echo "  docker-shell       - shell into running container"
	@echo "  curl-projects      - GET /api/v1/projects"
	@echo "  curl-upload        - POST sample upload (expects allure-results.zip in cwd)"
	@echo ""
	@echo "Variables:"
	@echo "  IMAGE, PORT, DATA_DIR, RUST_LOG"

.PHONY: build
build:
	cargo build

.PHONY: build-release
build-release:
	cargo build --release

.PHONY: run
run:
	@mkdir -p "$(DATA_DIR)"
	DATA_DIR="$(DATA_DIR)" LISTEN="0.0.0.0:$(PORT)" RUST_LOG="$(RUST_LOG)" cargo run

.PHONY: test
test:
	cargo test

.PHONY: fmt
fmt:
	cargo fmt

.PHONY: clippy
clippy:
	cargo clippy -- -D warnings

.PHONY: clean
clean:
	cargo clean

.PHONY: docker-build
docker-build:
	docker build -t "$(IMAGE)" .

.PHONY: docker-run
docker-run:
	@mkdir -p "$(DATA_DIR)"
	@docker rm -f "$(PROJECT)" >/dev/null 2>&1 || true
	docker run --name "$(PROJECT)" \
		-e "RUST_LOG=$(RUST_LOG)" \
		-p "$(PORT):8080" \
		-v "$(DATA_DIR):/data" \
		"$(IMAGE)"

.PHONY: docker-stop
docker-stop:
	@docker rm -f "$(PROJECT)" >/dev/null 2>&1 || true
	@echo "stopped: $(PROJECT)"

.PHONY: docker-logs
docker-logs:
	docker logs -f "$(PROJECT)"

.PHONY: docker-shell
docker-shell:
	docker exec -it "$(PROJECT)" /bin/bash

.PHONY: curl-projects
curl-projects:
	curl -sS "http://localhost:$(PORT)/api/v1/projects" | jq .

.PHONY: curl-upload
curl-upload:
	@test -f allure-results.zip || (echo "ERROR: allure-results.zip not found in current dir"; exit 1)
	curl -sS -X POST "http://localhost:$(PORT)/api/v1/projects/myproj/runs" \
		-F "results=@allure-results.zip" \
		-F 'meta={"branch":"main","commit":"abc123","trigger":"manual"}' | jq .
