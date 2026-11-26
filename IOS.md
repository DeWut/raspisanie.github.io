# SPBFTUIT для iOS

Приложение использует Capacitor и существующий интерфейс `miniapp/`. Это позволяет поддерживать общий интерфейс Android/iOS без отдельной переписи экранов.

## Сборка для симулятора

В репозитории настроен GitHub Actions workflow **Build iOS Simulator**:

1. Открой GitHub → Actions → **Build iOS Simulator**.
2. Запусти workflow для ветки `feature/smart-campus-step-1`.
3. Дождись успешной сборки.
4. Скачай artifact `SPBFTUIT-ios-simulator`.

Это unsigned-приложение для iOS Simulator. Оно не устанавливается напрямую на обычный iPhone.

## Локальная сборка

Нужен Mac с Xcode, Node.js 22 и CocoaPods (если его запросит Capacitor):

```bash
npm install
npm run mobile:prepare
npx cap add ios
npx cap sync ios
npx cap open ios
```

В Xcode можно запустить приложение на симуляторе. Для установки на собственный iPhone можно использовать Personal Team в Xcode с бесплатным Apple ID, если Xcode и устройство поддерживают такой сценарий; у бесплатной подписи есть ограничения по сроку действия и возможностям. Для распространения через TestFlight/App Store и стабильной подписанной IPA-сборки нужна платная Apple Developer Program и соответствующая подпись.

## Ограничения

- GitHub Actions workflow собирает только симуляторную версию без подписи.
- Он не создаёт IPA для установки на физическое устройство.
- Уведомления и другие нативные функции необходимо отдельно проверять на реальном iPhone; успешная сборка симулятора не подтверждает поведение на устройстве.
- Общий интерфейс остаётся веб-интерфейсом внутри Capacitor WebView, а не полностью нативным SwiftUI-приложением.
