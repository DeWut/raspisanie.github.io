// Browser/Telegram fallback. Android build replaces this file with the bundled Capacitor bridge.
window.SPBFTUITNative = {
  available: false,
  async requestNotificationPermission() { return { available: false, granted: false }; },
  async scheduleLessonReminders() { return { available: false, scheduled: 0 }; },
  async cancelLessonReminders() { return { available: false }; },
};
