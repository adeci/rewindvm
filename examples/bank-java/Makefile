# bank-java: bank in Java, with a JUnit 5 suite that marks its tests for
# Rewind. JUNIT is JUnit's console launcher, the standalone jar.
JUNIT ?= junit-platform-console-standalone.jar
PREFIX ?= /usr/local
SDK = dev/rewindvm/sdk/Rewind.java dev/rewindvm/sdk/RewindExtension.java

all: classes

classes: Account.java BankTest.java $(SDK)
	javac -d classes -cp $(JUNIT) Account.java BankTest.java $(SDK)

# The JUnit XML reports land in reports/.
check: classes
	java -jar $(JUNIT) execute --class-path classes --select-class BankTest \
		--reports-dir reports --disable-banner --details=tree

install: classes
	mkdir -p $(PREFIX)/share/bank-java
	cp -r classes $(PREFIX)/share/bank-java/

clean:
	rm -rf classes reports

.PHONY: all check install clean
