package io.github.mtmattia.rondine

import android.app.Application

class RondineApp : Application() {
    override fun onCreate() {
        super.onCreate()
        Tls.init(this)
        Notifier.createChannel(this)
        io.github.mtmattia.rondine.call.CallNotifications.createChannels(this)
        io.github.mtmattia.rondine.call.CallManager.init(this)
        io.github.mtmattia.rondine.push.PlatformPush.init(this)
        Repo.init(this)
    }
}
