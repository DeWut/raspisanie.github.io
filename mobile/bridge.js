import { Capacitor } from "@capacitor/core";
import { LocalNotifications } from "@capacitor/local-notifications";

const isNativeAndroid = () => Capacitor.isNativePlatform() && Capacitor.getPlatform() === "android";

window.SPBFTUITNative = {
  available: isNativeAndroid(),
  async requestNotificationPermission() {
    if (!isNativeAndroid()) return { available: false, granted: false };
    let permission = await LocalNotifications.checkPermissions();
    if (permission.display !== "granted") {
      permission = await LocalNotifications.requestPermissions();
    }
    return { available: true, granted: permission.display === "granted" };
  },
  async scheduleLessonReminders(items) {
    if (!isNativeAndroid()) return { available: false, scheduled: 0 };
    const permission = await this.requestNotificationPermission();
    if (!permission.granted) return { available: true, granted: false, scheduled: 0 };
    const pending = await LocalNotifications.getPending();
    if (pending.notifications.length) {
      await LocalNotifications.cancel({ notifications: pending.notifications });
    }
    if (!items.length) return { available: true, granted: true, scheduled: 0 };
    await LocalNotifications.schedule({
      notifications: items.map((item) => ({
        id: item.id,
        title: item.title,
        body: item.body,
        schedule: { at: new Date(item.at) },
        extra: { type: "lesson-reminder", date: item.date, groupId: item.groupId },
      })),
    });
    return { available: true, granted: true, scheduled: items.length };
  },
  async cancelLessonReminders() {
    if (!isNativeAndroid()) return { available: false };
    const pending = await LocalNotifications.getPending();
    if (pending.notifications.length) {
      await LocalNotifications.cancel({ notifications: pending.notifications });
    }
    return { available: true };
  },
};
