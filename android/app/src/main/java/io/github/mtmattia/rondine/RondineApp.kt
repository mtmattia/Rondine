package io.github.mtmattia.rondine

import android.app.Application

class RondineApp : Application() {
    override fun onCreate() {
        super.onCreate()
        Notifier.createChannel(this)
        Repo.init(this)
    }
}
