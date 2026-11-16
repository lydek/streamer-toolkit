-- The viewer's Twitch profile image for the stamp card overlay; only Twitch's image CDN is stored.
ALTER TABLE checkin_viewers ADD COLUMN profile_image_url TEXT;
