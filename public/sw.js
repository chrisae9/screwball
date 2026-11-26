self.addEventListener("push", (event) => {
  let payload = {};
  if (event.data) {
    try {
      payload = event.data.json();
    } catch {
      payload = { body: event.data.text() };
    }
  }

  const title = payload.title || "Your turn!";
  const options = {
    body: payload.body || "It is your turn in Scrabble.",
    icon: "/public/apple-touch-icon.png",
    badge: "/public/apple-touch-icon.png",
    tag: payload.tag || "screwball-turn",
    data: { url: payload.url || "/" },
  };

  event.waitUntil(Promise.all([
    self.registration.showNotification(title, options),
    self.clients.matchAll({type: "window", includeUncontrolled: true}).then(clients => {
      for (const client of clients) client.postMessage({type: "scrabble-activity"}, []);
    }),
  ]));
});

self.addEventListener("notificationclick", (event) => {
  event.notification.close();
  const rawUrl = event.notification.data && event.notification.data.url
    ? event.notification.data.url
    : "/";
  const url = new URL(rawUrl, self.location.origin).href;

  event.waitUntil(
    self.clients
      .matchAll({ type: "window", includeUncontrolled: true })
      .then((clients) => {
        for (const client of clients) {
          if (client.url === url && "focus" in client) {
            client.postMessage({type: "scrabble-activity"}, []);
            return client.focus();
          }
        }
        return self.clients.openWindow(url);
      }),
  );
});
