# Zombiebox developer shortcuts. Run `make` with no target to list them.
SHELL := /bin/bash
.DEFAULT_GOAL := help

# Android SDK: ANDROID_HOME wins, otherwise use a known local install.
ANDROID_HOME ?= $(firstword $(wildcard $(HOME)/Android/Sdk /tmp/zombiebox-android-tools/sdk))
export ANDROID_HOME

MOBILE_DIR := apps/mobile
GRADLE_APK := $(MOBILE_DIR)/android/app/build/outputs/apk/release/app-release.apk
APK := $(MOBILE_DIR)/build/zombiebox-android.apk

.PHONY: help apk build install run

help:
	@echo 'make apk      Build the sideloadable release APK -> $(APK)'
	@echo 'make build    Alias for `make apk`'
	@echo 'make install  Install $(APK) on a USB device with `adb install -r`'
	@echo 'make run      Run the browser playtest (cargo run --release --locked -p zombiebox-web)'

apk:
	@if [ -z "$(ANDROID_HOME)" ] || [ ! -d "$(ANDROID_HOME)/ndk" ]; then \
		echo 'Set ANDROID_HOME to an Android SDK with NDK 27.1.12297006 installed.' >&2; \
		exit 1; \
	fi
	cd $(MOBILE_DIR)/android && ./gradlew :app:assembleRelease
	mkdir -p $(MOBILE_DIR)/build
	cp $(GRADLE_APK) $(APK)
	@sha256sum $(APK)

build: apk

install:
	@test -f $(APK) || $(MAKE) apk
	adb install -r $(APK)

run:
	cargo run --release --locked -p zombiebox-web
