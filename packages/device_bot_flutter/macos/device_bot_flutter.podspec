Pod::Spec.new do |s|
  s.name = 'device_bot_flutter'
  s.version = '1.1.0'
  s.summary = 'Flutter discovery fallback for Device Bot.'
  s.description = s.summary
  s.homepage = 'https://github.com/ho-doan/device-bot'
  s.license = { :type => 'MIT' }
  s.author = { 'Device Bot' => 'device-bot@example.invalid' }
  s.source = { :path => '.' }
  s.source_files = 'Classes/**/*'
  s.dependency 'FlutterMacOS'
  s.platform = :osx, '10.15'
  s.swift_version = '5.0'
end
