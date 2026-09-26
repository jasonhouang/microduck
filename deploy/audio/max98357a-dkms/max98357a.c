/*
 * MAX98357A I2S Amplifier - Dummy ASoC Codec Driver
 *
 * MAX98357A is a simple I2S input Class-D amplifier with no configuration
 * interface. It just needs I2S data (BCLK, LRCK, DIN) and outputs to speaker.
 *
 * This driver provides the ASoC codec interface for the device tree.
 * If "sdmode-gpios" is specified in the device tree, the GPIO is driven
 * HIGH when a PCM stream opens (startup) and LOW when it closes (shutdown),
 * matching the active-LOW shutdown pin behaviour of the chip.
 */

#include <linux/module.h>
#include <linux/platform_device.h>
#include <linux/gpio/consumer.h>
#include <sound/soc.h>

struct max98357a_priv {
    struct gpio_desc *sdmode_gpio;
};

static int max98357a_dai_startup(struct snd_pcm_substream *substream,
                                  struct snd_soc_dai *dai)
{
    struct max98357a_priv *priv = dev_get_drvdata(dai->dev);

    if (priv->sdmode_gpio)
        gpiod_set_value_cansleep(priv->sdmode_gpio, 1); /* HIGH = amp ON  */
    return 0;
}

static void max98357a_dai_shutdown(struct snd_pcm_substream *substream,
                                    struct snd_soc_dai *dai)
{
    struct max98357a_priv *priv = dev_get_drvdata(dai->dev);

    if (priv->sdmode_gpio)
        gpiod_set_value_cansleep(priv->sdmode_gpio, 0); /* LOW = amp OFF */
}

static const struct snd_soc_dai_ops max98357a_dai_ops = {
    .startup  = max98357a_dai_startup,
    .shutdown = max98357a_dai_shutdown,
};

static struct snd_soc_dai_driver max98357a_dai = {
    .name = "max98357a-hifi",
    .ops  = &max98357a_dai_ops,
    .playback = {
        .stream_name = "Playback",
        .channels_min = 1,
        .channels_max = 2,
        .rates = SNDRV_PCM_RATE_8000_48000,
        .formats = SNDRV_PCM_FMTBIT_S16_LE | SNDRV_PCM_FMTBIT_S24_LE | SNDRV_PCM_FMTBIT_S32_LE,
    },
};

static struct snd_soc_component_driver max98357a_component = {
    .name = "max98357a-codec",
};

static int max98357a_probe(struct platform_device *pdev)
{
    struct max98357a_priv *priv;

    priv = devm_kzalloc(&pdev->dev, sizeof(*priv), GFP_KERNEL);
    if (!priv)
        return -ENOMEM;

    /* SD pin: active-LOW shutdown. flags=0 (GPIO_ACTIVE_HIGH in DT)
     * means logical 1 → physical HIGH → amplifier enabled. */
    priv->sdmode_gpio = devm_gpiod_get_optional(&pdev->dev, "sdmode",
                                                 GPIOD_OUT_LOW);
    if (IS_ERR(priv->sdmode_gpio))
        return PTR_ERR(priv->sdmode_gpio);

    platform_set_drvdata(pdev, priv);

    return snd_soc_register_component(&pdev->dev, &max98357a_component,
                                      &max98357a_dai, 1);
}

static int max98357a_remove(struct platform_device *pdev)
{
    snd_soc_unregister_component(&pdev->dev);
    return 0;
}

static const struct of_device_id max98357a_of_match[] = {
    { .compatible = "maxim,max98357a" },
    { }
};
MODULE_DEVICE_TABLE(of, max98357a_of_match);

static struct platform_driver max98357a_driver = {
    .driver = {
        .name = "max98357a",
        .of_match_table = max98357a_of_match,
    },
    .probe = max98357a_probe,
    .remove = max98357a_remove,
};

module_platform_driver(max98357a_driver);

MODULE_LICENSE("GPL");
MODULE_AUTHOR("Microduck Project");
MODULE_DESCRIPTION("Dummy ASoC codec driver for MAX98357A I2S amplifier");
MODULE_ALIAS("platform:max98357a");
